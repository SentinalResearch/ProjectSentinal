//! Validation and orchestration for immutable Session continuation.

use super::{SessionManager, SessionManagerError};
use protocol::{CaptureLevel, Session, SessionId};

const MAX_PATH_BYTES: usize = 4_096;
const MAX_PROJECT_ID_BYTES: usize = 256;

impl SessionManager {
    /// Continue one finished FULL Session as one distinct UI/FULL generation.
    pub async fn continue_session(
        &self,
        source_id: SessionId,
    ) -> Result<Session, SessionManagerError> {
        let _continuation = self.inner.continuation.lock().await;
        let store = self
            .inner
            .store
            .as_ref()
            .ok_or(SessionManagerError::StoreUnavailable)?;
        let lineage = store.ordered_lineage(source_id)?;
        let source = lineage
            .iter()
            .find(|session| session.id == source_id)
            .cloned()
            .ok_or(SessionManagerError::UnknownSession(source_id))?;
        require_continuable(&source)?;
        if let Some(successor) = store.direct_successor(source_id)? {
            return Ok(successor);
        }
        validate_continuation_metadata(&source)?;
        let root = lineage
            .first()
            .ok_or(SessionManagerError::UnknownSession(source_id))?;
        let title = root
            .title
            .clone()
            .or_else(|| Some(format!("Session {}", root.id.0)));
        self.spawn_continuation(source, title)
    }
}

fn require_continuable(session: &Session) -> Result<(), SessionManagerError> {
    if session.capture != CaptureLevel::Full {
        return Err(SessionManagerError::ContinuationCapture {
            session_id: session.id,
            capture: session.capture,
        });
    }
    if !session.state.is_finished() {
        return Err(SessionManagerError::ContinuationState {
            session_id: session.id,
            state: session.state,
        });
    }
    Ok(())
}

fn validate_continuation_metadata(session: &Session) -> Result<(), SessionManagerError> {
    for (field, value, maximum) in [
        ("shell", session.shell.as_str(), MAX_PATH_BYTES),
        ("cwd", session.cwd.as_str(), MAX_PATH_BYTES),
    ] {
        if value.is_empty() || value.len() > maximum {
            return Err(SessionManagerError::ContinuationMetadata {
                session_id: session.id,
                field,
            });
        }
    }
    if session
        .project_id
        .as_ref()
        .is_some_and(|project| project.0.is_empty() || project.0.len() > MAX_PROJECT_ID_BYTES)
    {
        return Err(SessionManagerError::ContinuationMetadata {
            session_id: session.id,
            field: "project_id",
        });
    }
    Ok(())
}
