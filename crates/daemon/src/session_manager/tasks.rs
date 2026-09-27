//! Background PTY reader and child-exit tasks.

use super::{Clock, SessionHandle};
use crate::{pty::PtyError, scrollback::Scrollback};
use protocol::{SessionId, SessionState};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::broadcast;

pub(super) fn spawn_reader(
    session_id: SessionId,
    mut reader: Box<dyn Read + Send>,
    parser: Arc<Mutex<vt100::Parser>>,
    scrollback: Arc<Mutex<Scrollback>>,
    output: broadcast::Sender<Vec<u8>>,
    log_path: PathBuf,
    log_file: std::fs::File,
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
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => return,
                Ok(count) => count,
                Err(error) => {
                    tracing::warn!(session = session_id.0, %error, "failed to read PTY output");
                    return;
                }
            };
            let bytes = &buffer[..count];
            if let Err(error) = encoder.write_all(bytes).and_then(|()| encoder.flush()) {
                tracing::error!(session = session_id.0, path = %log_path.display(), %error, "failed to append session log");
            }
            match parser.lock() {
                Ok(mut parser) => parser.process(bytes),
                Err(_) => {
                    tracing::error!(session = session_id.0, "screen lock poisoned");
                    return;
                }
            }
            match scrollback.lock() {
                Ok(mut scrollback) => scrollback.push(bytes),
                Err(_) => {
                    tracing::error!(session = session_id.0, "scrollback lock poisoned");
                    return;
                }
            }
            let _ = output.send(bytes.to_vec());
        }
    });
}

pub(super) fn spawn_exit_monitor(
    session_id: SessionId,
    handle: Arc<SessionHandle>,
    try_wait: Arc<dyn Fn() -> Result<Option<i32>, PtyError> + Send + Sync>,
    clock: Clock,
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
                            if let Err(error) = session.transition(target, clock()) {
                                tracing::error!(session = session_id.0, %error, "failed to record child exit");
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
