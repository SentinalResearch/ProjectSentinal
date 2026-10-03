//! Attach and lifecycle-gated PTY operations.

use super::{
    lock_session, validate_size, Attachment, SessionHandle, SessionManager, SessionManagerError,
};
use crate::audit::AuditRecord;
use protocol::{DaemonMessage, SessionId, SessionState};
use std::{
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::broadcast;

use super::capture::{
    ensure_session_operation, require_capture, require_operation, CaptureOperation,
};

impl SessionManager {
    /// Snapshot the current screen and subscribe to subsequent output.
    pub fn attach(&self, session_id: SessionId) -> Result<Attachment, SessionManagerError> {
        let handle = self.handle(session_id)?;
        require_capture(&handle, CaptureOperation::Attach)?;
        snapshot_and_subscribe(
            &handle.output,
            &handle.structured,
            handle.attached.clone(),
            || {
                Ok(handle
                    .parser
                    .lock()
                    .map_err(|_| SessionManagerError::Lock {
                        resource: "screen",
                        session_id,
                    })?
                    .screen()
                    .state_formatted())
            },
        )
    }

    /// Return the retained line-oriented scrollback.
    pub fn scrollback(&self, session_id: SessionId) -> Result<Vec<String>, SessionManagerError> {
        let handle = self.handle(session_id)?;
        let lines = handle
            .scrollback
            .lock()
            .map_err(|_| SessionManagerError::Lock {
                resource: "scrollback",
                session_id,
            })?
            .lines();
        Ok(lines)
    }

    /// Resize a session's PTY and in-memory screen.
    pub fn resize(
        &self,
        session_id: SessionId,
        cols: u16,
        rows: u16,
    ) -> Result<(), SessionManagerError> {
        validate_size(cols, rows)?;
        let handle = self.operation_handle(session_id, CaptureOperation::Resize)?;
        (handle.resize)(cols, rows)?;
        handle
            .parser
            .lock()
            .map_err(|_| SessionManagerError::Lock {
                resource: "screen",
                session_id,
            })?
            .screen_mut()
            .set_size(rows, cols);
        Ok(())
    }

    /// Write terminal input and audit metadata after a successful PTY write.
    pub fn write_input(
        &self,
        session_id: SessionId,
        bytes: &[u8],
        client_addr: &str,
    ) -> Result<(), SessionManagerError> {
        let handle = self.operation_handle(session_id, CaptureOperation::Input)?;
        let mut writer = handle
            .writer
            .lock()
            .map_err(|_| SessionManagerError::Lock {
                resource: "PTY writer",
                session_id,
            })?;
        let write_result = writer.write_all(bytes).and_then(|()| writer.flush());
        drop(writer);
        if let Err(source) = write_result {
            if is_closed_device_error(&source) {
                handle.pty_closed.store(true, Ordering::SeqCst);
                self.reconcile_child_exit(&handle)?;
                return closed_pty_error(&handle, CaptureOperation::Input);
            }
            return Err(SessionManagerError::File {
                operation: "write PTY input for",
                path: PathBuf::from(format!("session-{}", session_id.0)),
                source,
            });
        }
        self.inner.audit.append(&AuditRecord {
            timestamp: (self.inner.config.clock)(),
            client_addr: client_addr.to_string(),
            session_id,
            byte_count: bytes.len(),
        })?;
        Ok(())
    }

    /// Kill a session and mark it interrupted without removing its history.
    pub fn kill(&self, session_id: SessionId) -> Result<(), SessionManagerError> {
        let handle = self.operation_handle(session_id, CaptureOperation::Kill)?;
        (handle.kill)()?;
        handle.pty_closed.store(true, Ordering::SeqCst);
        let mut session = lock_session(&handle)?;
        if !session.state.is_finished() {
            session.transition(SessionState::Interrupted, (self.inner.config.clock)())?;
            if let Some(store) = &self.inner.store {
                store.update_session(&session)?;
            }
        }
        Ok(())
    }

    /// Stop every live daemon-owned PTY before the daemon exits; returns how many.
    ///
    /// A restart interrupts FULL Sessions anyway (until Reliability R1), and a
    /// live PTY reader would otherwise keep the process alive after SIGTERM.
    /// Reduced-capture Sessions own no PTY and are left for restart recovery.
    pub fn shutdown(&self) -> usize {
        let Ok(handles) = self.handles() else {
            tracing::error!("session map lock poisoned during shutdown");
            return 0;
        };
        let mut interrupted = 0;
        for handle in handles {
            let live = lock_session(&handle).is_ok_and(|session| {
                session.capture == protocol::CaptureLevel::Full && !session.state.is_finished()
            });
            if !live || handle.pty_closed.load(Ordering::SeqCst) {
                continue;
            }
            match self.kill(handle.id) {
                Ok(()) => interrupted += 1,
                Err(error) => {
                    tracing::warn!(session = handle.id.0, %error, "failed to stop Session at shutdown");
                }
            }
        }
        interrupted
    }

    fn reconcile_child_exit(&self, handle: &SessionHandle) -> Result<(), SessionManagerError> {
        let Some(code) = (handle.try_wait)()? else {
            return Ok(());
        };
        let target = if code == 0 {
            SessionState::Completed
        } else {
            SessionState::Failed
        };
        let mut session = lock_session(handle)?;
        if session.state.is_finished() {
            return Ok(());
        }
        session.exit_code = Some(code);
        session.transition(target, (self.inner.config.clock)())?;
        if let Some(store) = &self.inner.store {
            store.update_session(&session)?;
        }
        let _ = handle.structured.send(DaemonMessage::SessionUpdated {
            session: session.clone(),
        });
        Ok(())
    }

    fn operation_handle(
        &self,
        session_id: SessionId,
        operation: CaptureOperation,
    ) -> Result<Arc<SessionHandle>, SessionManagerError> {
        match self.handle(session_id) {
            Ok(handle) => {
                require_operation(&handle, operation)?;
                Ok(handle)
            }
            Err(SessionManagerError::UnknownSession(_)) => {
                let session = self.known_session(session_id)?;
                ensure_session_operation(&session, true, operation)?;
                Err(SessionManagerError::PtyClosed {
                    session_id,
                    state: session.state,
                    operation: operation.description(),
                })
            }
            Err(error) => Err(error),
        }
    }
}

fn closed_pty_error(
    handle: &SessionHandle,
    operation: CaptureOperation,
) -> Result<(), SessionManagerError> {
    let session = lock_session(handle)?;
    if session.state.is_finished() {
        Err(SessionManagerError::TerminalStateDenied {
            session_id: session.id,
            state: session.state,
            operation: operation.description(),
        })
    } else {
        Err(SessionManagerError::PtyClosed {
            session_id: session.id,
            state: session.state,
            operation: operation.description(),
        })
    }
}

fn is_closed_device_error(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::UnexpectedEof
            | std::io::ErrorKind::WriteZero
    ) || error.raw_os_error() == Some(5)
}

fn snapshot_and_subscribe(
    output_sender: &broadcast::Sender<Vec<u8>>,
    structured_sender: &broadcast::Sender<DaemonMessage>,
    confirmation: Arc<AtomicBool>,
    snapshot: impl FnOnce() -> Result<Vec<u8>, SessionManagerError>,
) -> Result<Attachment, SessionManagerError> {
    // Subscribe before taking the snapshot. Output written while the parser
    // lock is contended is then either present in the snapshot or waiting in
    // this receiver, never lost between the two operations.
    let output = output_sender.subscribe();
    let structured = structured_sender.subscribe();
    let screen = snapshot()?;
    Ok(Attachment {
        screen,
        output,
        structured,
        confirmation,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn attachment_subscribes_before_snapshotting() {
        let (output, _) = broadcast::channel(4);
        let (structured, _) = broadcast::channel(4);
        let mut attachment = snapshot_and_subscribe(
            &output,
            &structured,
            Arc::new(AtomicBool::new(false)),
            || {
                let _ = output.send(b"during snapshot".to_vec());
                Ok(b"screen".to_vec())
            },
        )
        .unwrap();

        assert_eq!(attachment.screen, b"screen");
        assert_eq!(attachment.output.try_recv().unwrap(), b"during snapshot");
    }
}
