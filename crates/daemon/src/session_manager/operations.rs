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
            ScreenSource {
                session_id,
                parser: handle.parser.clone(),
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

/// Full terminal reset (RIS): a viewer drops everything it has drawn, so a
/// redraw that follows starts from a clean terminal.
pub const RESET_TERMINAL: &[u8] = b"\x1bc";

/// The Session's screen, for redrawing a viewer that fell behind.
#[derive(Clone)]
pub struct ScreenSource {
    session_id: SessionId,
    parser: Arc<std::sync::Mutex<vt100::Parser>>,
}

impl ScreenSource {
    #[cfg(test)]
    pub(crate) fn new(session_id: SessionId, parser: Arc<std::sync::Mutex<vt100::Parser>>) -> Self {
        Self { session_id, parser }
    }

    /// The current screen, with `output` replaced by a receiver that starts
    /// exactly after it. The PTY reader publishes output while holding the
    /// screen lock, so no byte is both in the screen and in the receiver, or
    /// in neither.
    pub fn snapshot(
        &self,
        output: &broadcast::Receiver<Vec<u8>>,
    ) -> Result<(Vec<u8>, broadcast::Receiver<Vec<u8>>), SessionManagerError> {
        let parser = self.parser.lock().map_err(|_| SessionManagerError::Lock {
            resource: "screen",
            session_id: self.session_id,
        })?;
        Ok((parser.screen().state_formatted(), output.resubscribe()))
    }

    /// After a lag: a reset followed by the current screen, and a receiver
    /// that continues from that screen.
    pub fn resync(
        &self,
        output: &broadcast::Receiver<Vec<u8>>,
    ) -> Result<(Vec<u8>, broadcast::Receiver<Vec<u8>>), SessionManagerError> {
        let (screen, receiver) = self.snapshot(output)?;
        Ok(([RESET_TERMINAL, &screen].concat(), receiver))
    }
}

fn snapshot_and_subscribe(
    output_sender: &broadcast::Sender<Vec<u8>>,
    structured_sender: &broadcast::Sender<DaemonMessage>,
    confirmation: Arc<AtomicBool>,
    screen_source: ScreenSource,
) -> Result<Attachment, SessionManagerError> {
    let structured = structured_sender.subscribe();
    let (screen, output) = screen_source.snapshot(&output_sender.subscribe())?;
    Ok(Attachment {
        screen,
        output,
        structured,
        confirmation,
        screen_source,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn source() -> ScreenSource {
        ScreenSource {
            session_id: SessionId(1),
            parser: Arc::new(std::sync::Mutex::new(vt100::Parser::new(4, 20, 0))),
        }
    }

    /// What the PTY reader does: draw and publish under the screen lock.
    fn produce(source: &ScreenSource, output: &broadcast::Sender<Vec<u8>>, bytes: &[u8]) {
        let mut parser = source.parser.lock().unwrap();
        parser.process(bytes);
        let _ = output.send(bytes.to_vec());
    }

    #[test]
    fn attachment_output_starts_exactly_after_the_screen() {
        let (output, _) = broadcast::channel(4);
        let (structured, _) = broadcast::channel(4);
        let source = source();
        produce(&source, &output, b"before");
        let mut attachment = snapshot_and_subscribe(
            &output,
            &structured,
            Arc::new(AtomicBool::new(false)),
            source.clone(),
        )
        .unwrap();
        produce(&source, &output, b" after");

        assert!(String::from_utf8_lossy(&attachment.screen).contains("before"));
        assert_eq!(attachment.output.try_recv().unwrap(), b" after");
        assert!(attachment.output.try_recv().is_err());
    }

    #[test]
    fn a_lagged_viewer_is_redrawn_and_continues_from_the_redraw() {
        let (output, _) = broadcast::channel(2);
        let (structured, _) = broadcast::channel(2);
        let source = source();
        let mut attachment = snapshot_and_subscribe(
            &output,
            &structured,
            Arc::new(AtomicBool::new(false)),
            source.clone(),
        )
        .unwrap();
        for line in ["one\r\n", "two\r\n", "three\r\n", "four"] {
            produce(&source, &output, line.as_bytes());
        }
        assert!(matches!(
            attachment.output.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(2))
        ));

        let (redraw, mut receiver) = source.resync(&attachment.output).unwrap();
        produce(&source, &output, b"!");

        assert!(redraw.starts_with(RESET_TERMINAL));
        let screen = String::from_utf8_lossy(&redraw);
        for line in ["one", "two", "three", "four"] {
            assert!(screen.contains(line), "{line} missing from the redraw");
        }
        assert_eq!(receiver.try_recv().unwrap(), b"!");
        assert!(receiver.try_recv().is_err());
    }
}
