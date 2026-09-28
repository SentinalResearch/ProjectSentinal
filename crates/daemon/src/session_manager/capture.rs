//! Capture-level gate for daemon-owned PTY operations.

use super::{lock_session, SessionHandle, SessionManagerError};
use protocol::CaptureLevel;

pub(super) fn require_capture(
    handle: &SessionHandle,
    operation: CaptureOperation,
) -> Result<(), SessionManagerError> {
    let capture = lock_session(handle)?.capture;
    ensure_capture(capture, operation)
}

#[derive(Debug, Clone, Copy)]
pub(super) enum CaptureOperation {
    Attach,
    Input,
    Resize,
    Kill,
}

pub(super) fn ensure_capture(
    capture: CaptureLevel,
    operation: CaptureOperation,
) -> Result<(), SessionManagerError> {
    let capabilities = capture.capabilities();
    let (allowed, description) = match operation {
        CaptureOperation::Attach => (capabilities.can_attach, "attach"),
        CaptureOperation::Input => (capabilities.can_send_input, "accept terminal input"),
        CaptureOperation::Resize => (capabilities.can_resize, "resize"),
        CaptureOperation::Kill => (capabilities.can_kill_daemon_pty, "kill a daemon-owned PTY"),
    };
    if allowed {
        Ok(())
    } else {
        Err(SessionManagerError::CaptureDenied {
            capture,
            operation: description,
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
