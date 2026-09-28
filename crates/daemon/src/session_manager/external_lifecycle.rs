//! Liveness, restart recovery, and ending of reduced-capture Sessions.

use super::{
    external::lock_reports, lock_session, SessionHandle, SessionManager, SessionManagerError,
};
use protocol::{
    CaptureLevel, DaemonMessage, Event, EventKind, Session, SessionId, SessionOrigin, SessionState,
    Timestamp,
};

impl SessionManager {
    /// End reduced-capture Sessions whose root process exited or was replaced.
    pub fn check_external_processes(&self) -> Result<Vec<SessionId>, SessionManagerError> {
        let mut ended = Vec::new();
        for handle in self.handles()? {
            let session = lock_session(&handle)?.clone();
            if session.capture == CaptureLevel::Full || session.state.is_finished() {
                continue;
            }
            if !self.same_process(&session) {
                self.end_external(&handle)?;
                ended.push(session.id);
            }
        }
        ended.sort();
        Ok(ended)
    }

    /// After a restart, resume verified live external Sessions and end the rest.
    pub fn recover_external_sessions(
        &self,
        stored: Vec<Session>,
    ) -> Result<(), SessionManagerError> {
        let now = (self.inner.config.clock)();
        for mut session in stored {
            if session.capture == CaptureLevel::Full || session.state.is_finished() {
                continue;
            }
            if self.same_process(&session) {
                // A Command running across the restart cannot be finished later.
                let running_process = session.origin == SessionOrigin::Adopted;
                if running_process && session.state == SessionState::Disconnected {
                    session.transition(SessionState::Running, now)?;
                } else if !running_process
                    && (session.state.is_busy() || session.state == SessionState::Disconnected)
                {
                    session.transition(SessionState::Idle, now)?;
                }
                if let Some(store) = &self.inner.store {
                    store.update_session(&session)?;
                }
                self.insert_external(session)?;
            } else {
                let event = ended_event(self, &mut session, now)?;
                if let Some(store) = &self.inner.store {
                    store.update_session(&session)?;
                    store.insert_event(&event)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn same_process(&self, session: &Session) -> bool {
        let (Some(pid), Some(started_at)) = (session.pid, session.process_started_at) else {
            return false;
        };
        let owner = self.inner.probe.owner_uid();
        self.inner.probe.identify(pid).is_some_and(|identity| {
            Some(identity.uid) == owner && identity.started_at == started_at
        })
    }

    pub(super) fn end_external(&self, handle: &SessionHandle) -> Result<(), SessionManagerError> {
        let mut reports = lock_reports(handle)?;
        reports.forget_running();
        if let Ok(mut tracker) = handle.tracker.lock() {
            let _ = tracker.abandon();
        }
        let now = (self.inner.config.clock)();
        let (session, event) = {
            let mut session = lock_session(handle)?;
            let event = ended_event(self, &mut session, now)?;
            (session.clone(), event)
        };
        if let Some(store) = &self.inner.store {
            store.update_session(&session)?;
            store.insert_event(&event)?;
        }
        let _ = handle.structured.send(DaemonMessage::Event { event });
        self.observe_reduced(DaemonMessage::SessionUpdated {
            session: session.clone(),
        });
        let _ = handle
            .structured
            .send(DaemonMessage::SessionUpdated { session });
        Ok(())
    }
}

/// Mark an external Session ended with an unknown exit code (approved in 1b.0).
pub(super) fn ended_event(
    manager: &SessionManager,
    session: &mut Session,
    at: Timestamp,
) -> Result<Event, SessionManagerError> {
    let command_id = session.current_command;
    session.transition(SessionState::Completed, at)?;
    session.exit_code = None;
    Ok(Event {
        id: manager.inner.tracker_ids.event(),
        session_id: session.id,
        command_id,
        at,
        kind: EventKind::ProcessExited {
            pid: session.pid.unwrap_or_default(),
            exit_code: None,
        },
    })
}
