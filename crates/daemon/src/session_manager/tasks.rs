//! Background PTY reader and child-exit tasks.

use super::{Clock, SessionHandle};
use crate::{
    osc::{OscParser, ShellSignal},
    pty::PtyError,
    tracker::TrackerUpdate,
};
use protocol::{DaemonMessage, SessionId, SessionState};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use store::Store;

pub(super) fn spawn_reader(
    session_id: SessionId,
    mut reader: Box<dyn Read + Send>,
    handle: Arc<SessionHandle>,
    log_path: PathBuf,
    log_file: std::fs::File,
    store: Option<Arc<Store>>,
    clock: Clock,
) {
    tokio::task::spawn_blocking(move || {
        let mut encoder = match zstd::stream::write::Encoder::new(log_file, 0) {
            Ok(encoder) => encoder.auto_finish(),
            Err(error) => {
                tracing::error!(session = session_id.0, path = %log_path.display(), %error, "failed to start compressed session log");
                return;
            }
        };
        let mut buffer = [0_u8; 8192];
        let mut osc = OscParser::new();
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => {
                    let trailing = osc.finish();
                    if !trailing.is_empty() {
                        if let Err(error) =
                            encoder.write_all(&trailing).and_then(|()| encoder.flush())
                        {
                            tracing::error!(session = session_id.0, path = %log_path.display(), %error, "failed to append trailing session log");
                        }
                        match handle.parser.lock() {
                            Ok(mut parser) => parser.process(&trailing),
                            Err(_) => {
                                tracing::error!(session = session_id.0, "screen lock poisoned");
                            }
                        }
                        match handle.scrollback.lock() {
                            Ok(mut scrollback) => scrollback.push(&trailing),
                            Err(_) => {
                                tracing::error!(session = session_id.0, "scrollback lock poisoned");
                            }
                        }
                        let _ = handle.output.send(trailing);
                    }
                    return;
                }
                Ok(count) => count,
                Err(error) => {
                    tracing::warn!(session = session_id.0, %error, "failed to read PTY output");
                    return;
                }
            };
            let parsed = osc.push(&buffer[..count]);
            let bytes = parsed.output.as_slice();
            if bytes.is_empty() {
                continue;
            }
            if let Err(error) = encoder.write_all(bytes).and_then(|()| encoder.flush()) {
                tracing::error!(session = session_id.0, path = %log_path.display(), %error, "failed to append session log");
            }
            match handle.parser.lock() {
                Ok(mut parser) => parser.process(bytes),
                Err(_) => {
                    tracing::error!(session = session_id.0, "screen lock poisoned");
                    return;
                }
            }
            match handle.scrollback.lock() {
                Ok(mut scrollback) => scrollback.push(bytes),
                Err(_) => {
                    tracing::error!(session = session_id.0, "scrollback lock poisoned");
                    return;
                }
            }
            let _ = handle.output.send(bytes.to_vec());
            let mut prompt_ready = false;
            for signal in parsed.signals {
                prompt_ready |= signal == ShellSignal::PromptStart;
                let _ = apply_signal(session_id, &handle, store.as_deref(), signal, clock());
            }
            if prompt_ready {
                type_initial_command(session_id, &handle);
            }
        }
    });
}

/// Type the Watch initial Command at most once, after the first prompt.
fn type_initial_command(session_id: SessionId, handle: &SessionHandle) {
    let pending = match handle.initial_command.lock() {
        Ok(mut pending) => pending.take(),
        Err(_) => {
            tracing::error!(session = session_id.0, "initial command lock poisoned");
            return;
        }
    };
    let Some(line) = pending else {
        return;
    };
    match handle.writer.lock() {
        Ok(mut writer) => {
            if let Err(error) = writer.write_all(&line).and_then(|()| writer.flush()) {
                tracing::error!(session = session_id.0, %error, "failed to type initial command");
            }
        }
        Err(_) => tracing::error!(session = session_id.0, "PTY writer lock poisoned"),
    }
}

/// Apply one signal, persist and broadcast the result, and return it.
pub(super) fn apply_signal(
    session_id: SessionId,
    handle: &SessionHandle,
    store: Option<&Store>,
    signal: crate::osc::ShellSignal,
    at: protocol::Timestamp,
) -> Option<TrackerUpdate> {
    let update = match (handle.tracker.lock(), handle.session.lock()) {
        (Ok(mut tracker), Ok(mut session)) => match tracker.handle(&mut session, signal, at) {
            Ok(update) => (update, session.clone()),
            Err(error) => {
                tracing::warn!(session = session_id.0, %error, "failed to apply shell signal");
                return None;
            }
        },
        _ => {
            tracing::error!(session = session_id.0, "command tracking lock poisoned");
            return None;
        }
    };
    let (update, session) = update;
    if let Some(store) = store {
        let result = if let Some(command) = &update.command {
            if command.finished_at.is_some() {
                store.finish_command(command)
            } else {
                store.insert_command(command)
            }
        } else {
            Ok(())
        }
        .and_then(|()| {
            update
                .event
                .as_ref()
                .map_or(Ok(()), |event| store.insert_event(event))
        })
        .and_then(|()| {
            if update.session_changed {
                store.update_session(&session)
            } else {
                Ok(())
            }
        });
        if let Err(error) = result {
            tracing::error!(session = session_id.0, %error, "failed to persist shell signal");
        }
    }
    let reduced = session.capture != protocol::CaptureLevel::Full;
    if let Some(command) = &update.command {
        let message = DaemonMessage::CommandUpdated {
            command: command.clone(),
        };
        let _ = handle.structured.send(message.clone());
        if reduced {
            let _ = handle.observations.send(message);
        }
    }
    if let Some(event) = &update.event {
        let message = DaemonMessage::Event {
            event: event.clone(),
        };
        let _ = handle.structured.send(message.clone());
        if reduced {
            let _ = handle.observations.send(message);
        }
    }
    if update.session_changed {
        let message = DaemonMessage::SessionUpdated { session };
        let _ = handle.structured.send(message.clone());
        if reduced {
            let _ = handle.observations.send(message);
        }
    }
    Some(update)
}

pub(super) fn spawn_exit_monitor(
    session_id: SessionId,
    handle: Arc<SessionHandle>,
    try_wait: Arc<dyn Fn() -> Result<Option<i32>, PtyError> + Send + Sync>,
    clock: Clock,
    store: Option<Arc<Store>>,
) {
    tokio::spawn(async move {
        loop {
            match try_wait() {
                Ok(Some(code)) => {
                    let target = if code == 0 {
                        SessionState::Completed
                    } else {
                        SessionState::Failed
                    };
                    match handle.session.lock() {
                        Ok(mut session) if !session.state.is_finished() => {
                            session.exit_code = Some(code);
                            if let Err(error) = session.transition(target, clock()) {
                                tracing::error!(session = session_id.0, %error, "failed to record child exit");
                            } else {
                                if let Some(store) = &store {
                                    if let Err(error) = store.update_session(&session) {
                                        tracing::error!(session = session_id.0, %error, "failed to persist child exit");
                                    }
                                }
                                // Attached clients stop waiting once the shell is gone.
                                let _ = handle.structured.send(DaemonMessage::SessionUpdated {
                                    session: session.clone(),
                                });
                            }
                        }
                        Ok(_) => {}
                        Err(_) => tracing::error!(session = session_id.0, "session lock poisoned"),
                    }
                    return;
                }
                Ok(None) => tokio::time::sleep(Duration::from_millis(10)).await,
                Err(error) => {
                    tracing::error!(session = session_id.0, %error, "failed to poll child");
                    return;
                }
            }
        }
    });
}
