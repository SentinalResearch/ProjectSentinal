//! Deterministic conversion of shell signals into Commands and Events.

use crate::osc::ShellSignal;
use protocol::{
    Command, CommandId, Event, EventId, EventKind, MachineId, ProtocolError, Session, SessionState,
    Timestamp,
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Process-wide command and event ID allocation shared by session trackers.
#[derive(Debug, Clone, Default)]
pub struct TrackerIds {
    command: Arc<AtomicU64>,
    event: Arc<AtomicU64>,
}

impl TrackerIds {
    /// Allocate IDs beginning at the supplied values.
    pub fn starting_at(command: u64, event: u64) -> Self {
        TrackerIds {
            command: Arc::new(AtomicU64::new(command)),
            event: Arc::new(AtomicU64::new(event)),
        }
    }

    fn command(&self) -> CommandId {
        CommandId(self.command.fetch_add(1, Ordering::Relaxed))
    }

    pub(crate) fn event(&self) -> EventId {
        EventId(self.event.fetch_add(1, Ordering::Relaxed))
    }
}

/// Structured changes caused by one shell signal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackerUpdate {
    /// Command that started or finished.
    pub command: Option<Command>,
    /// Event corresponding to the command lifecycle change.
    pub event: Option<Event>,
    /// Whether the Session fields changed.
    pub session_changed: bool,
}

/// Errors applying shell signals to Session state.
#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    /// A Session state transition was invalid.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

/// Tracks the command currently running in one Session.
#[derive(Debug)]
pub struct CommandTracker {
    machine_id: MachineId,
    ids: TrackerIds,
    command_text: String,
    cwd: String,
    current: Option<Command>,
}

impl CommandTracker {
    /// Create an idle tracker using a Session's initial working directory.
    pub fn new(machine_id: MachineId, cwd: impl Into<String>, ids: TrackerIds) -> Self {
        CommandTracker {
            machine_id,
            ids,
            command_text: String::new(),
            cwd: cwd.into(),
            current: None,
        }
    }

    /// Apply one shell signal at an injected timestamp.
    pub fn handle(
        &mut self,
        session: &mut Session,
        signal: ShellSignal,
        at: Timestamp,
    ) -> Result<TrackerUpdate, TrackerError> {
        match signal {
            ShellSignal::CommandText(text) => {
                self.command_text = text.clone();
                if let Some(command) = self.current.as_mut() {
                    command.text = text;
                    return Ok(TrackerUpdate {
                        command: Some(command.clone()),
                        ..TrackerUpdate::default()
                    });
                }
            }
            ShellSignal::Cwd(cwd) => {
                self.cwd = cwd.clone();
                if session.cwd != cwd {
                    session.cwd = cwd;
                    session.updated_at = at;
                    return Ok(TrackerUpdate {
                        session_changed: true,
                        ..TrackerUpdate::default()
                    });
                }
            }
            ShellSignal::CommandStart => return self.start(session, at),
            ShellSignal::CommandEnd(exit_code) => return self.finish(session, exit_code, at),
            ShellSignal::PromptStart | ShellSignal::OutputStart => {}
        }
        Ok(TrackerUpdate::default())
    }

    /// Forget the running Command without finishing it (its end was never reported).
    pub fn abandon(&mut self) -> Option<Command> {
        self.command_text.clear();
        self.current.take()
    }

    /// Finish a running Command when its shell exits before reporting an end marker.
    pub fn finish_on_shell_exit(
        &mut self,
        session: &mut Session,
        exit_code: i32,
        at: Timestamp,
    ) -> Result<TrackerUpdate, TrackerError> {
        if self.current.is_none() {
            return Ok(TrackerUpdate::default());
        }
        self.finish(session, exit_code, at)
    }

    fn start(
        &mut self,
        session: &mut Session,
        at: Timestamp,
    ) -> Result<TrackerUpdate, TrackerError> {
        if self.current.is_some() {
            tracing::warn!(
                session = session.id.0,
                "ignored command start while another command is active"
            );
            return Ok(TrackerUpdate::default());
        }
        let command = Command::start(
            self.ids.command(),
            session.id,
            self.machine_id.clone(),
            self.command_text.clone(),
            self.cwd.clone(),
            at,
        );
        session.transition(SessionState::Running, at)?;
        session.current_command = Some(command.id);
        let event = Event {
            id: self.ids.event(),
            session_id: session.id,
            command_id: Some(command.id),
            at,
            kind: EventKind::CommandStarted {
                text: command.text.clone(),
            },
        };
        self.current = Some(command.clone());
        Ok(TrackerUpdate {
            command: Some(command),
            event: Some(event),
            session_changed: true,
        })
    }

    fn finish(
        &mut self,
        session: &mut Session,
        exit_code: i32,
        at: Timestamp,
    ) -> Result<TrackerUpdate, TrackerError> {
        let Some(mut command) = self.current.take() else {
            tracing::warn!(
                session = session.id.0,
                exit_code,
                "ignored command end without a command start"
            );
            return Ok(TrackerUpdate::default());
        };
        command.finish(exit_code, at)?;
        session.transition(SessionState::Idle, at)?;
        let event = Event {
            id: self.ids.event(),
            session_id: session.id,
            command_id: Some(command.id),
            at,
            kind: EventKind::CommandFinished {
                exit_code,
                duration_ms: command.duration(at),
            },
        };
        self.command_text.clear();
        Ok(TrackerUpdate {
            command: Some(command),
            event: Some(event),
            session_changed: true,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{CommandStatus, DurationMs, SessionId};

    fn setup() -> (Session, CommandTracker) {
        let machine = MachineId("test-machine".into());
        let mut session = Session::new(
            SessionId(8),
            machine.clone(),
            "/bin/sh",
            "/work",
            Timestamp(0),
        );
        session
            .transition(SessionState::Idle, Timestamp(1))
            .unwrap();
        let tracker = CommandTracker::new(machine, "/work", TrackerIds::starting_at(10, 20));
        (session, tracker)
    }

    fn start(tracker: &mut CommandTracker, session: &mut Session, text: &str, at: i64) {
        tracker
            .handle(
                session,
                ShellSignal::CommandText(text.into()),
                Timestamp(at),
            )
            .unwrap();
        tracker
            .handle(session, ShellSignal::CommandStart, Timestamp(at))
            .unwrap();
    }

    #[test]
    fn successful_command_has_injected_duration_and_events() {
        let (mut session, mut tracker) = setup();
        start(&mut tracker, &mut session, "cargo build", 1_000);

        let update = tracker
            .handle(&mut session, ShellSignal::CommandEnd(0), Timestamp(32_000))
            .unwrap();

        let command = update.command.unwrap();
        assert_eq!(command.status(), CommandStatus::Succeeded);
        assert_eq!(command.duration(Timestamp(99_000)), DurationMs(31_000));
        assert_eq!(session.state, SessionState::Idle);
        assert_eq!(session.current_command, None);
        assert!(matches!(
            update.event.unwrap().kind,
            EventKind::CommandFinished {
                exit_code: 0,
                duration_ms: DurationMs(31_000)
            }
        ));
    }

    #[test]
    fn exit_130_is_a_failed_command() {
        let (mut session, mut tracker) = setup();
        start(&mut tracker, &mut session, "pytest", 100);
        let command = tracker
            .handle(&mut session, ShellSignal::CommandEnd(130), Timestamp(200))
            .unwrap()
            .command
            .unwrap();
        assert_eq!(command.status(), CommandStatus::Failed);
        assert_eq!(command.exit_code, Some(130));
    }

    #[test]
    fn shell_exit_finishes_the_current_command_but_is_quiet_at_a_prompt() {
        let (mut session, mut tracker) = setup();
        assert_eq!(
            tracker
                .finish_on_shell_exit(&mut session, 0, Timestamp(10))
                .unwrap(),
            TrackerUpdate::default()
        );

        start(&mut tracker, &mut session, "exec python train.py", 100);
        let update = tracker
            .finish_on_shell_exit(&mut session, 137, Timestamp(200))
            .unwrap();
        let command = update.command.unwrap();
        assert_eq!(command.exit_code, Some(137));
        assert_eq!(command.finished_at, Some(Timestamp(200)));
        assert_eq!(session.current_command, None);
        assert_eq!(session.state, SessionState::Idle);
    }

    #[test]
    fn two_commands_run_in_sequence_with_distinct_ids() {
        let (mut session, mut tracker) = setup();
        start(&mut tracker, &mut session, "true", 10);
        let first = tracker
            .handle(&mut session, ShellSignal::CommandEnd(0), Timestamp(20))
            .unwrap()
            .command
            .unwrap();
        start(&mut tracker, &mut session, "false", 30);
        let second = tracker
            .handle(&mut session, ShellSignal::CommandEnd(1), Timestamp(40))
            .unwrap()
            .command
            .unwrap();
        assert_ne!(first.id, second.id);
        assert_eq!(second.text, "false");
    }

    #[test]
    fn command_end_without_start_is_ignored() {
        let (mut session, mut tracker) = setup();
        let update = tracker
            .handle(&mut session, ShellSignal::CommandEnd(0), Timestamp(10))
            .unwrap();
        assert_eq!(update, TrackerUpdate::default());
        assert_eq!(session.state, SessionState::Idle);
    }

    #[test]
    fn cwd_signal_updates_session_and_next_command() {
        let (mut session, mut tracker) = setup();
        tracker
            .handle(&mut session, ShellSignal::Cwd("/tmp".into()), Timestamp(5))
            .unwrap();
        start(&mut tracker, &mut session, "pwd", 10);
        let command = tracker
            .handle(&mut session, ShellSignal::CommandEnd(0), Timestamp(11))
            .unwrap()
            .command
            .unwrap();
        assert_eq!(session.cwd, "/tmp");
        assert_eq!(command.cwd, "/tmp");
    }
}
