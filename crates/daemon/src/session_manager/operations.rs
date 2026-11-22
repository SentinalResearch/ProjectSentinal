//! Attach and lifecycle-gated PTY operations.

use super::{lock_session, Attachment, SessionHandle, SessionManager, SessionManagerError};
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

use super::capture::{ensure_session_operation, require_operation, CaptureOperation};

impl SessionManager {
    /// Snapshot the current screen and subscribe to subsequent output.
    pub fn attach(&self, session_id: SessionId) -> Result<Attachment, SessionManagerError> {
        self.attach_with_history(session_id, false)
    }

    /// [`attach`](Self::attach), with the scrollback in front of the screen
    /// (and in front of any redraw after a lag) when `history` is set.
    pub fn attach_with_history(
        &self,
        session_id: SessionId,
        history: bool,
    ) -> Result<Attachment, SessionManagerError> {
        self.attach_viewer(session_id, history, super::ViewerOptions::default())
    }
}

/// The screen and live output for one new viewer of `handle`.
pub(super) fn snapshot_attachment(handle: &SessionHandle, history: bool) -> Attachment {
    snapshot_and_subscribe(
        &handle.structured,
        handle.attached.clone(),
        ScreenSource {
            screen: handle.screen.clone(),
            history,
        },
    )
}

impl SessionManager {
    /// The Session's history and screen as text lines, oldest first.
    pub fn scrollback(&self, session_id: SessionId) -> Result<Vec<String>, SessionManagerError> {
        Ok(self.handle(session_id)?.screen.lines())
    }

    /// Resize a Session at once, as an old client's `RESIZE` does (T3).
    pub fn resize(
        &self,
        session_id: SessionId,
        cols: u16,
        rows: u16,
    ) -> Result<(), SessionManagerError> {
        self.legacy_resize(session_id, None, cols, rows, "local")
    }

    /// Audit terminal input metadata, then write the input to the PTY.
    pub fn write_input(
        &self,
        session_id: SessionId,
        bytes: &[u8],
        client_addr: &str,
    ) -> Result<(), SessionManagerError> {
        let handle = self.operation_handle(session_id, CaptureOperation::Input)?;
        // Audit first and fail closed: input that cannot be recorded never
        // reaches the shell. A record therefore means "input was attempted".
        self.inner.audit.append(&AuditRecord {
            timestamp: (self.inner.config.clock)(),
            client_addr: client_addr.to_string(),
            session_id,
            byte_count: bytes.len(),
            resize: None,
        })?;
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
        // The watcher learns only that input arrived, never what it was: a
        // question that leaves the screen afterwards was answered (X18).
        super::tasks::apply_observation(
            session_id,
            &handle,
            self.inner.store.as_deref(),
            watcher::Observation::input((self.inner.config.clock)()),
        );
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
            // Viewers see the Session end now; its Command ends when the exit
            // monitor sees the shell go.
            let _ = handle.structured.send(DaemonMessage::SessionUpdated {
                session: session.clone(),
            });
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

    pub(super) fn operation_handle(
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
    screen: crate::screen::Screen,
    /// Put the history in front of every screen (T2.4).
    history: bool,
}

impl ScreenSource {
    #[cfg(test)]
    pub(crate) fn new(screen: crate::screen::Screen) -> Self {
        Self {
            screen,
            history: false,
        }
    }

    /// The current screen and a receiver for the output that follows it:
    /// no byte is in both, or in neither ([`crate::screen`]).
    pub fn snapshot(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        self.screen.snapshot(self.history)
    }

    /// After a lag: a reset followed by the current screen, and a receiver
    /// that continues from that screen.
    pub fn resync(&self) -> (Vec<u8>, broadcast::Receiver<Vec<u8>>) {
        let (screen, receiver) = self.snapshot();
        ([RESET_TERMINAL, &screen].concat(), receiver)
    }
}

fn snapshot_and_subscribe(
    structured_sender: &broadcast::Sender<DaemonMessage>,
    confirmation: Arc<AtomicBool>,
    screen_source: ScreenSource,
) -> Attachment {
    let structured = structured_sender.subscribe();
    let (screen, output) = screen_source.snapshot();
    Attachment {
        screen,
        output,
        structured,
        confirmation,
        screen_source,
        viewer: None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::screen::{Screen, ScreenSize};

    fn source(rows: u16, history: bool, queue: usize) -> ScreenSource {
        let (output, _) = broadcast::channel(queue);
        let size = ScreenSize {
            cols: 20,
            rows,
            history_lines: 100,
            attach_history_lines: 100,
        };
        ScreenSource {
            screen: Screen::spawn("test", size, output),
            history,
        }
    }

    /// What the PTY reader does.
    fn produce(source: &ScreenSource, bytes: &[u8]) {
        source.screen.feed(bytes.to_vec());
    }

    #[test]
    fn a_history_attachment_carries_lines_scrolled_off_the_screen() {
        let source = source(2, true, 4);
        produce(&source, b"first\r\nsecond\r\nthird\r\nfourth");
        let plain = ScreenSource {
            history: false,
            ..source.clone()
        };

        let (with_history, _) = source.snapshot();
        let (screen_only, _) = plain.snapshot();

        assert!(String::from_utf8_lossy(&with_history).contains("first"));
        assert!(!String::from_utf8_lossy(&screen_only).contains("first"));
        let (redraw, _) = source.resync();
        assert!(redraw.starts_with(RESET_TERMINAL));
        assert!(String::from_utf8_lossy(&redraw).contains("first"));
    }

    #[test]
    fn attachment_output_starts_exactly_after_the_screen() {
        let (structured, _) = broadcast::channel(4);
        let source = source(4, false, 4);
        produce(&source, b"before");
        let mut attachment = snapshot_and_subscribe(
            &structured,
            Arc::new(AtomicBool::new(false)),
            source.clone(),
        );
        produce(&source, b" after");

        assert!(String::from_utf8_lossy(&attachment.screen).contains("before"));
        assert_eq!(attachment.output.blocking_recv().unwrap(), b" after");
        assert!(attachment.output.try_recv().is_err());
    }

    #[test]
    fn a_lagged_viewer_is_redrawn_and_continues_from_the_redraw() {
        let (structured, _) = broadcast::channel(2);
        let source = source(4, false, 2);
        let mut attachment = snapshot_and_subscribe(
            &structured,
            Arc::new(AtomicBool::new(false)),
            source.clone(),
        );
        for line in ["one\r\n", "two\r\n", "three\r\n", "four"] {
            produce(&source, line.as_bytes());
        }
        // Wait until the screen thread has drawn everything.
        assert!(source.screen.lines().contains(&"four".to_string()));
        assert!(matches!(
            attachment.output.try_recv(),
            Err(broadcast::error::TryRecvError::Lagged(2))
        ));

        let (redraw, mut receiver) = source.resync();
        produce(&source, b"!");

        assert!(redraw.starts_with(RESET_TERMINAL));
        let screen = String::from_utf8_lossy(&redraw);
        for line in ["one", "two", "three", "four"] {
            assert!(screen.contains(line), "{line} missing from the redraw");
        }
        assert_eq!(receiver.blocking_recv().unwrap(), b"!");
        assert!(receiver.try_recv().is_err());
    }
}
