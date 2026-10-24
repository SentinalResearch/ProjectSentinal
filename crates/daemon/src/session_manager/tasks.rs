//! Background PTY reader and child-exit tasks.

use super::{Clock, SessionHandle};
use crate::{
    osc::{OscParser, ShellSignal},
    pty::PtyError,
    tracker::TrackerUpdate,
};
use protocol::{DaemonMessage, Event, EventKind, SessionId, SessionState};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use store::Store;

/// Compressed output log that the PTY reader appends to.
pub(super) struct OutputLog {
    /// Log path, for error messages.
    pub(super) path: PathBuf,
    /// Open owner-only log file.
    pub(super) file: std::fs::File,
}

pub(super) fn spawn_reader(
    session_id: SessionId,
    mut reader: Box<dyn Read + Send>,
    handle: Arc<SessionHandle>,
    log: OutputLog,
    mut osc: OscParser,
    store: Option<Arc<Store>>,
    clock: Clock,
) {
    let OutputLog {
        path: log_path,
        file: log_file,
    } = log;
    tokio::task::spawn_blocking(move || {
        let mut encoder = match zstd::stream::write::Encoder::new(log_file, 0) {
            Ok(encoder) => encoder.auto_finish(),
            Err(error) => {
                tracing::error!(session = session_id.0, path = %log_path.display(), %error, "failed to start compressed session log");
                return;
            }
        };
        let mut budget =
            super::log_budget::LogBudget::new(super::log_budget::SESSION_LOG_CAP_BYTES);
        let mut buffer = [0_u8; 8192];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => {
                    let trailing = osc.finish();
                    if !trailing.is_empty() {
                        if let Err(error) = save(&mut encoder, &mut budget, &trailing) {
                            tracing::error!(session = session_id.0, path = %log_path.display(), %error, "failed to append trailing session log");
                        }
                        // Published under the screen lock: see `ScreenSource::snapshot`.
                        match handle.parser.lock() {
                            Ok(mut parser) => {
                                parser.process(&trailing);
                                let _ = handle.output.send(trailing.clone());
                            }
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
                        apply_output(session_id, &handle, store.as_deref(), &trailing, clock());
                    }
                    finish_output(session_id, &handle, store.as_deref(), clock());
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
            if let Err(error) = save(&mut encoder, &mut budget, bytes) {
                crate::self_health::COUNTERS
                    .error(protocol::observability::HealthLabel::ErrorLogWrite);
                tracing::error!(session = session_id.0, path = %log_path.display(), %error, "failed to append session log");
            }
            // Published under the screen lock: see `ScreenSource::snapshot`.
            match handle.parser.lock() {
                Ok(mut parser) => {
                    parser.process(bytes);
                    let _ = handle.output.send(bytes.to_vec());
                }
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
            let mut prompt_ready = false;
            for signal in parsed.signals {
                prompt_ready |= signal == ShellSignal::PromptStart;
                let _ = apply_signal(session_id, &handle, store.as_deref(), signal, clock());
            }
            apply_output(session_id, &handle, store.as_deref(), bytes, clock());
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
    publish_tracker_update(session_id, handle, store, &update, session, at);
    Some(update)
}

fn publish_tracker_update(
    session_id: SessionId,
    handle: &SessionHandle,
    store: Option<&Store>,
    update: &TrackerUpdate,
    session: protocol::Session,
    at: protocol::Timestamp,
) {
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
        let _ = handle.events.send(event.clone());
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
    if let (Some(command), Some(event)) = (&update.command, &update.event) {
        let kind = match event.kind {
            EventKind::CommandStarted { .. } => {
                Some(watcher::ObservationKind::CommandStarted(command.clone()))
            }
            EventKind::CommandFinished { .. } => {
                Some(watcher::ObservationKind::CommandFinished(command.clone()))
            }
            _ => None,
        };
        if let Some(kind) = kind {
            apply_observation(session_id, handle, store, watcher::Observation { at, kind });
        }
    }
}

pub(super) fn apply_output(
    session_id: SessionId,
    handle: &SessionHandle,
    store: Option<&Store>,
    bytes: &[u8],
    at: protocol::Timestamp,
) {
    let lines = match handle.line_framer.lock() {
        Ok(mut framer) => framer.push(bytes),
        Err(_) => {
            tracing::error!(session = session_id.0, "watcher line-framer lock poisoned");
            return;
        }
    };
    for line in lines {
        apply_observation(
            session_id,
            handle,
            store,
            watcher::Observation::output(at, line),
        );
    }
}

pub(super) fn finish_output(
    session_id: SessionId,
    handle: &SessionHandle,
    store: Option<&Store>,
    at: protocol::Timestamp,
) {
    let line = match handle.line_framer.lock() {
        Ok(mut framer) => framer.finish(),
        Err(_) => {
            tracing::error!(session = session_id.0, "watcher line-framer lock poisoned");
            return;
        }
    };
    if let Some(line) = line {
        apply_observation(
            session_id,
            handle,
            store,
            watcher::Observation::output(at, line),
        );
    }
}

pub(super) fn apply_observation(
    session_id: SessionId,
    handle: &SessionHandle,
    store: Option<&Store>,
    observation: watcher::Observation,
) {
    let update = match handle.watcher.lock() {
        Ok(mut engine) => engine.observe(&observation),
        Err(_) => {
            tracing::error!(session = session_id.0, "watcher engine lock poisoned");
            return;
        }
    };
    let training = match &update.adapter_state {
        Some(watcher::AdapterState::Training(state)) => state.summary(),
        _ => None,
    };
    let (session, command_id, session_changed, reduced) = match handle.session.lock() {
        Ok(mut session) => {
            let mut session_changed = session.health.as_ref() != Some(&update.health);
            if session_changed {
                session.health = Some(update.health);
            }
            // Keep the last summary once the run stops printing training numbers.
            if training.is_some() && session.training != training {
                session.training = training;
                session_changed = true;
            }
            (
                session.clone(),
                session.current_command,
                session_changed,
                session.capture != protocol::CaptureLevel::Full,
            )
        }
        Err(_) => {
            tracing::error!(
                session = session_id.0,
                "Session lock poisoned during watcher update"
            );
            return;
        }
    };
    let events = update
        .events
        .into_iter()
        .map(|kind| Event {
            id: handle.watcher_ids.event(),
            session_id,
            command_id,
            at: observation.at,
            kind,
        })
        .collect::<Vec<_>>();
    if let Some(store) = store {
        for event in &events {
            if let Err(error) = store.insert_event(event) {
                tracing::error!(session = session_id.0, %error, "failed to persist watcher Event");
            }
        }
        if session_changed {
            if let Err(error) = store.update_session(&session) {
                tracing::error!(session = session_id.0, %error, "failed to persist Session Health and training");
            }
        }
    }
    for event in events {
        let _ = handle.events.send(event.clone());
        let message = DaemonMessage::Event { event };
        let _ = handle.structured.send(message.clone());
        if reduced {
            let _ = handle.observations.send(message);
        }
    }
    if session_changed {
        let message = DaemonMessage::SessionUpdated { session };
        let _ = handle.structured.send(message.clone());
        if reduced {
            let _ = handle.observations.send(message);
        }
    }
}

pub(super) fn apply_health_event(
    session_id: SessionId,
    handle: &SessionHandle,
    store: Option<&Store>,
    at: protocol::Timestamp,
    event: &EventKind,
) {
    let health = match handle.watcher.lock() {
        Ok(mut engine) => engine.observe_existing_event(at, event),
        Err(_) => {
            tracing::error!(session = session_id.0, "watcher engine lock poisoned");
            return;
        }
    };
    let (session, changed, reduced) = match handle.session.lock() {
        Ok(mut session) => {
            let changed = session.health.as_ref() != Some(&health);
            if changed {
                session.health = Some(health);
            }
            (
                session.clone(),
                changed,
                session.capture != protocol::CaptureLevel::Full,
            )
        }
        Err(_) => {
            tracing::error!(
                session = session_id.0,
                "Session lock poisoned during Health update"
            );
            return;
        }
    };
    if !changed {
        return;
    }
    if let Some(store) = store {
        if let Err(error) = store.update_session(&session) {
            tracing::error!(session = session_id.0, %error, "failed to persist Session Health and training");
        }
    }
    let message = DaemonMessage::SessionUpdated { session };
    let _ = handle.structured.send(message.clone());
    if reduced {
        let _ = handle.observations.send(message);
    }
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
                    handle
                        .pty_closed
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    let at = clock();
                    let tracker_update = match (handle.tracker.lock(), handle.session.lock()) {
                        (Ok(mut tracker), Ok(mut session)) => Some(
                            tracker
                                .finish_on_shell_exit(&mut session, code, at)
                                .map(|update| (update, session.clone())),
                        ),
                        _ => {
                            tracing::error!(
                                session = session_id.0,
                                "command tracking lock poisoned during child exit"
                            );
                            None
                        }
                    };
                    match tracker_update {
                        Some(Ok((update, session))) => publish_tracker_update(
                            session_id,
                            &handle,
                            store.as_deref(),
                            &update,
                            session,
                            at,
                        ),
                        Some(Err(error)) => {
                            tracing::warn!(session = session_id.0, %error, "failed to reconcile current Command on child exit");
                        }
                        None => {}
                    }
                    let target = if code == 0 {
                        SessionState::Completed
                    } else {
                        SessionState::Failed
                    };
                    match handle.session.lock() {
                        Ok(mut session) if !session.state.is_finished() => {
                            session.exit_code = Some(code);
                            if let Err(error) = session.transition(target, at) {
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

/// Save `bytes` within this Session's log budget (F9).
fn save(
    encoder: &mut impl std::io::Write,
    budget: &mut super::log_budget::LogBudget,
    bytes: &[u8],
) -> std::io::Result<()> {
    use super::log_budget::{Admit, CAP_MARKER};
    match budget.admit(bytes.len()) {
        Admit::Write => encoder.write_all(bytes)?,
        Admit::Marker => {
            tracing::warn!("a Session reached its saved-output limit; live output continues");
            encoder.write_all(CAP_MARKER)?;
        }
        Admit::Skip => return Ok(()),
    }
    encoder.flush()
}
