//! A session: one persistent terminal (PTY + shell) that survives UI disconnects.
//!
//! # State machine
//!
//! ```text
//!   CREATED ──► IDLE ◄──────► RUNNING ◄──► WAITING
//!      │         │  ▲            │  ▲         │
//!      │         │  └────────────┼──┼─────────┘   (command ends → IDLE)
//!      │         │               ▼  │
//!      │         │            STALLED
//!      ▼         ▼
//!   FAILED   COMPLETED / FAILED / INTERRUPTED     (end states: the shell is gone)
//!
//!   DISCONNECTED: the viewer lost contact with the daemon. Any live state can
//!   become DISCONNECTED, and DISCONNECTED can return to any state except CREATED.
//! ```
//!
//! What each state means:
//! - `CREATED`      — registered, shell not started yet
//! - `IDLE`         — shell is at a prompt
//! - `RUNNING`      — a command is executing
//! - `WAITING`      — the running command seems to be waiting for input
//! - `STALLED`      — the running command shows no output and no resource activity
//! - `COMPLETED`    — the shell exited with code 0
//! - `FAILED`       — the shell exited non-zero or could not start
//! - `INTERRUPTED`  — the session was killed (by the user or a signal)
//! - `DISCONNECTED` — state unknown because the daemon is unreachable

use crate::{CommandId, MachineId, ProjectId, ProtocolError, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// Lifecycle state of a session. See the module docs for the diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionState {
    /// Registered, shell not started yet.
    Created,
    /// Shell is at a prompt.
    Idle,
    /// A command is executing.
    Running,
    /// The running command seems to be waiting for input.
    Waiting,
    /// The running command shows no output and no resource activity.
    Stalled,
    /// The shell exited with code 0.
    Completed,
    /// The shell exited non-zero or could not start.
    Failed,
    /// The session was killed.
    Interrupted,
    /// State unknown: the daemon is unreachable.
    Disconnected,
}

impl SessionState {
    /// Every state, in declaration order. Handy for exhaustive tests.
    pub const ALL: [SessionState; 9] = [
        SessionState::Created,
        SessionState::Idle,
        SessionState::Running,
        SessionState::Waiting,
        SessionState::Stalled,
        SessionState::Completed,
        SessionState::Failed,
        SessionState::Interrupted,
        SessionState::Disconnected,
    ];

    /// True for end states: the shell is gone and nothing can change any more.
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            SessionState::Completed | SessionState::Failed | SessionState::Interrupted
        )
    }

    /// True while a command is in progress (running, waiting or stalled).
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            SessionState::Running | SessionState::Waiting | SessionState::Stalled
        )
    }

    /// Whether a session may move from `self` to `to`.
    ///
    /// Moving to the same state is not a transition and returns `false`.
    pub fn can_transition_to(self, to: SessionState) -> bool {
        use SessionState::*;
        if self == to || self.is_finished() || to == Created {
            return false;
        }
        match self {
            Created => matches!(to, Idle | Failed | Interrupted | Disconnected),
            Idle => matches!(
                to,
                Running | Completed | Failed | Interrupted | Disconnected
            ),
            Running | Waiting | Stalled => to != Created, // any live or end state
            Disconnected => true,                         // reconnect reveals the real state
            Completed | Failed | Interrupted => false,
        }
    }
}

/// A persistent terminal on one machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Session {
    /// ID, unique on this machine.
    pub id: SessionId,
    /// Machine the session runs on.
    pub machine_id: MachineId,
    /// Project this session belongs to, if any.
    pub project_id: Option<ProjectId>,
    /// Optional user-given name, e.g. `"Qwen Training"`.
    pub title: Option<String>,
    /// Shell program, e.g. `"/bin/zsh"`.
    pub shell: String,
    /// Current working directory, as last reported by the shell.
    pub cwd: String,
    /// Current lifecycle state.
    pub state: SessionState,
    /// The command running right now, if any.
    pub current_command: Option<CommandId>,
    /// When the session was created.
    pub created_at: Timestamp,
    /// When `state` last changed.
    pub updated_at: Timestamp,
}

impl Session {
    /// A new session in the `CREATED` state.
    pub fn new(
        id: SessionId,
        machine_id: MachineId,
        shell: impl Into<String>,
        cwd: impl Into<String>,
        at: Timestamp,
    ) -> Self {
        Session {
            id,
            machine_id,
            project_id: None,
            title: None,
            shell: shell.into(),
            cwd: cwd.into(),
            state: SessionState::Created,
            current_command: None,
            created_at: at,
            updated_at: at,
        }
    }

    /// Move to a new state, or return `InvalidTransition` and change nothing.
    ///
    /// Setting the state it already has is a no-op and succeeds.
    /// When the session stops being busy, `current_command` is cleared.
    pub fn transition(&mut self, to: SessionState, at: Timestamp) -> Result<(), ProtocolError> {
        if self.state == to {
            return Ok(());
        }
        if !self.state.can_transition_to(to) {
            return Err(ProtocolError::InvalidTransition {
                from: self.state,
                to,
            });
        }
        self.state = to;
        self.updated_at = at;
        if !to.is_busy() && to != SessionState::Disconnected {
            self.current_command = None;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use SessionState::*;

    fn test_session() -> Session {
        Session::new(SessionId(1), "rtx-pc".into(), "/bin/zsh", "~", Timestamp(0))
    }

    fn session_in(state: SessionState) -> Session {
        let mut s = test_session();
        s.state = state;
        s
    }

    #[test]
    fn new_session_starts_created() {
        assert_eq!(test_session().state, Created);
    }

    #[test]
    fn created_session_becomes_idle_when_shell_is_ready() {
        let mut s = test_session();
        s.transition(Idle, Timestamp(10)).unwrap();
        assert_eq!(s.state, Idle);
        assert_eq!(s.updated_at, Timestamp(10));
    }

    #[test]
    fn created_session_cannot_start_running_directly() {
        let mut s = test_session();
        let err = s.transition(Running, Timestamp(10)).unwrap_err();
        assert_eq!(
            err,
            ProtocolError::InvalidTransition {
                from: Created,
                to: Running
            }
        );
    }

    #[test]
    fn failed_transition_changes_nothing() {
        let mut s = test_session();
        let before = s.clone();
        let _ = s.transition(Running, Timestamp(10));
        assert_eq!(s, before);
    }

    #[test]
    fn running_session_can_be_interrupted() {
        let mut s = session_in(Running);
        s.transition(Interrupted, Timestamp(5)).unwrap();
        assert_eq!(s.state, Interrupted);
    }

    #[test]
    fn finished_states_cannot_go_anywhere() {
        for end in [Completed, Failed, Interrupted] {
            for to in SessionState::ALL {
                assert!(
                    !end.can_transition_to(to),
                    "{end:?} should not transition to {to:?}"
                );
            }
        }
    }

    #[test]
    fn nothing_can_return_to_created() {
        for from in SessionState::ALL {
            assert!(!from.can_transition_to(Created), "{from:?} -> Created");
        }
    }

    #[test]
    fn idle_cannot_be_waiting_or_stalled_without_a_command() {
        assert!(!Idle.can_transition_to(Waiting));
        assert!(!Idle.can_transition_to(Stalled));
    }

    #[test]
    fn busy_states_can_move_between_each_other() {
        for from in [Running, Waiting, Stalled] {
            for to in [Running, Waiting, Stalled] {
                if from != to {
                    assert!(from.can_transition_to(to), "{from:?} -> {to:?}");
                }
            }
        }
    }

    #[test]
    fn any_live_state_can_become_disconnected() {
        for from in [Created, Idle, Running, Waiting, Stalled] {
            assert!(
                from.can_transition_to(Disconnected),
                "{from:?} -> Disconnected"
            );
        }
    }

    #[test]
    fn disconnected_session_can_reconnect_to_real_state() {
        for to in [
            Idle,
            Running,
            Waiting,
            Stalled,
            Completed,
            Failed,
            Interrupted,
        ] {
            assert!(Disconnected.can_transition_to(to), "Disconnected -> {to:?}");
        }
    }

    #[test]
    fn transition_to_same_state_is_a_no_op() {
        let mut s = session_in(Idle);
        s.transition(Idle, Timestamp(99)).unwrap();
        assert_eq!(s.updated_at, Timestamp(0), "timestamp must not change");
    }

    #[test]
    fn returning_to_idle_clears_current_command() {
        let mut s = session_in(Running);
        s.current_command = Some(CommandId(7));
        s.transition(Idle, Timestamp(1)).unwrap();
        assert_eq!(s.current_command, None);
    }

    #[test]
    fn disconnecting_keeps_current_command() {
        let mut s = session_in(Running);
        s.current_command = Some(CommandId(7));
        s.transition(Disconnected, Timestamp(1)).unwrap();
        assert_eq!(s.current_command, Some(CommandId(7)));
    }

    #[test]
    fn busy_and_finished_flags_are_correct() {
        assert!(Running.is_busy() && Waiting.is_busy() && Stalled.is_busy());
        assert!(!Idle.is_busy());
        assert!(Completed.is_finished() && Failed.is_finished() && Interrupted.is_finished());
        assert!(!Disconnected.is_finished());
    }
}
