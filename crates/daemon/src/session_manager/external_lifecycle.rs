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
            if self.same_process(&session) {
                continue;
            }
            // A `nohup`-style wrapper may exit while its child keeps writing.
            if session.origin == SessionOrigin::Adopted && self.adopted_descendant_alive(&handle)? {
                continue;
            }
            self.end_external(&handle)?;
            ended.push(session.id);
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
                let adopted_log = session
                    .log
                    .clone()
                    .filter(|_| session.origin == SessionOrigin::Adopted);
                // Resume from the persisted cursor so the log is not replayed.
                let adopted_tail = adopted_log
                    .as_ref()
                    .map(|log| self.open_session_tail(session.id, log));
                if session.origin == SessionOrigin::Adopted {
                    session.capture = CaptureLevel::Process;
                }
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
                let session_id = session.id;
                let shell_hook = session.origin == SessionOrigin::ShellHook;
                let argv = (session.origin == SessionOrigin::Adopted)
                    .then(|| {
                        session
                            .pid
                            .and_then(|pid| self.inner.probe.details(pid).command)
                    })
                    .flatten();
                self.insert_external(session, argv)?;
                match (adopted_tail, &adopted_log) {
                    (Some(Ok(tail)), Some(log)) => self.activate_log_tail(session_id, tail, log)?,
                    (Some(Err(error)), Some(log)) => {
                        self.warn_log_unavailable(session_id, log, &error)?;
                    }
                    _ => {}
                }
                if shell_hook {
                    self.allow_initial_shell_sequence_gap(session_id)?;
                }
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
        // Drain already-written log bytes before the Session is marked ended.
        self.finish_log_tail(handle)?;
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
        let _ = handle.events.send(event.clone());
        let event_message = DaemonMessage::Event { event };
        let _ = handle.structured.send(event_message.clone());
        self.observe_reduced(event_message);
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
