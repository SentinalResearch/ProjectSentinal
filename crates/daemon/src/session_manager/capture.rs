//! Capture-level gate for daemon-owned PTY operations.

use super::{lock_session, SessionHandle, SessionManagerError};
use protocol::{CaptureLevel, Session};

pub(super) fn require_capture(
    handle: &SessionHandle,
    operation: CaptureOperation,
) -> Result<(), SessionManagerError> {
    let capture = lock_session(handle)?.capture;
    ensure_capture(capture, operation)
}

pub(super) fn require_operation(
    handle: &SessionHandle,
    operation: CaptureOperation,
) -> Result<(), SessionManagerError> {
    let session = lock_session(handle)?;
    ensure_session_operation(
        &session,
        handle.pty_closed.load(std::sync::atomic::Ordering::SeqCst),
        operation,
    )
}

pub(super) fn ensure_session_operation(
    session: &Session,
    pty_closed: bool,
    operation: CaptureOperation,
) -> Result<(), SessionManagerError> {
    ensure_capture(session.capture, operation)?;
    if operation.mutates_pty() && session.state.is_finished() {
        return Err(SessionManagerError::TerminalStateDenied {
            session_id: session.id,
            state: session.state,
            operation: operation.description(),
        });
    }
    if operation.mutates_pty() && pty_closed {
        return Err(SessionManagerError::PtyClosed {
            session_id: session.id,
            state: session.state,
            operation: operation.description(),
        });
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(super) enum CaptureOperation {
    Attach,
    Input,
    Resize,
    Kill,
}

impl CaptureOperation {
    pub(super) fn description(self) -> &'static str {
        match self {
            CaptureOperation::Attach => "attach",
            CaptureOperation::Input => "accept terminal input",
            CaptureOperation::Resize => "resize",
            CaptureOperation::Kill => "kill a daemon-owned PTY",
        }
    }

    fn mutates_pty(self) -> bool {
        !matches!(self, CaptureOperation::Attach)
    }
}

pub(super) fn ensure_capture(
    capture: CaptureLevel,
    operation: CaptureOperation,
) -> Result<(), SessionManagerError> {
    let capabilities = capture.capabilities();
    let allowed = match operation {
        CaptureOperation::Attach => capabilities.can_attach,
        CaptureOperation::Input => capabilities.can_send_input,
        CaptureOperation::Resize => capabilities.can_resize,
        CaptureOperation::Kill => capabilities.can_kill_daemon_pty,
    };
    if allowed {
        Ok(())
    } else {
        Err(SessionManagerError::CaptureDenied {
            capture,
            operation: operation.description(),
        })
    }
}

#[cfg(test)]
impl super::SessionManager {
    /// Give an existing Session a reduced origin/capture so boundary tests can
    /// exercise enforcement before 1b.B/1b.C can register such Sessions.
    pub(crate) fn set_capture_for_test(
        &self,
        session_id: protocol::SessionId,
        origin: protocol::SessionOrigin,
        capture: CaptureLevel,
    ) -> Result<(), SessionManagerError> {
        let handle = self.handle(session_id)?;
        let mut session = lock_session(&handle)?;
        session.origin = origin;
        session.capture = capture;
        session.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{ensure_capture, CaptureOperation};
    use protocol::CaptureLevel;

    #[test]
    fn every_reduced_capture_rejects_all_remote_pty_operations() {
        for capture in [
            CaptureLevel::Commands,
            CaptureLevel::ProcessLog,
            CaptureLevel::Process,
        ] {
            for operation in [
                CaptureOperation::Attach,
                CaptureOperation::Input,
                CaptureOperation::Resize,
                CaptureOperation::Kill,
            ] {
                let error = ensure_capture(capture, operation).unwrap_err();
                assert!(error.to_string().contains(&capture.to_string()));
            }
        }
    }

    #[test]
    fn full_capture_allows_all_remote_pty_operations() {
        for operation in [
            CaptureOperation::Attach,
            CaptureOperation::Input,
            CaptureOperation::Resize,
            CaptureOperation::Kill,
        ] {
            assert!(ensure_capture(CaptureLevel::Full, operation).is_ok());
        }
    }
}
