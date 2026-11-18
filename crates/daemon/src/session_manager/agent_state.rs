//! An agent inside a Session reporting what it is doing (Milestone 6.4).
//!
//! Accepted only from a process the OS says runs inside that Session's
//! process tree, only for live FULL Sessions, at most once per 500 ms. A
//! report is presentation: it never changes lifecycle state, is never
//! written to the store, and clears when the Command or Session ends.

use super::{lock_session, SessionManager, SessionManagerError};
use protocol::{AgentReportedState, AgentStatus, CaptureLevel, DaemonMessage, Session, SessionId};

/// Shortest gap between two accepted reports for one Session.
const MIN_REPORT_GAP_MS: i64 = 500;

impl SessionManager {
    /// Record what an agent running in `session_id` says it is doing.
    ///
    /// A report arriving within 500 ms of the last accepted one is dropped
    /// quietly and the Session is returned unchanged.
    pub fn report_agent_state(
        &self,
        session_id: SessionId,
        status: AgentStatus,
        detail: Option<String>,
        caller_pid: Option<u32>,
    ) -> Result<Session, SessionManagerError> {
        let caller = caller_pid.ok_or(SessionManagerError::AgentCallerUnknown)?;
        let handle = self.handle(session_id)?;
        let root = {
            let session = lock_session(&handle)?;
            if session.capture != CaptureLevel::Full || session.state.is_finished() {
                return Err(SessionManagerError::AgentReportsNotTaken(session_id));
            }
            session.pid
        };
        // The process-tree walk can be slow; never hold the Session lock for it.
        let inside = root.is_some_and(|root| self.inner.probe.descends_from(caller, root));
        if !inside {
            return Err(SessionManagerError::AgentOutsideSession {
                pid: caller,
                session_id,
            });
        }

        let now = (self.inner.config.clock)();
        {
            let mut last =
                self.inner
                    .agent_reports
                    .lock()
                    .map_err(|_| SessionManagerError::Lock {
                        resource: "agent reports",
                        session_id,
                    })?;
            if last
                .get(&session_id)
                .is_some_and(|at| now.0.saturating_sub(at.0) < MIN_REPORT_GAP_MS)
            {
                return Ok(lock_session(&handle)?.clone());
            }
            last.insert(session_id, now);
        }

        let session = {
            let mut session = lock_session(&handle)?;
            session.agent_state = Some(Box::new(AgentReportedState {
                status,
                detail,
                reported_at: now,
            }));
            session.clone()
        };
        let _ = handle.structured.send(DaemonMessage::SessionUpdated {
            session: session.clone(),
        });
        Ok(session)
    }
}
