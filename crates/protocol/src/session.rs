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

use crate::{CommandId, MachineId, ProjectId, ProtocolError, SessionId, SessionLog, Timestamp};
use serde::{Deserialize, Serialize};

/// How a Session entered Sentinal.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionOrigin {
    /// Created through the ordinary UI/API flow.
    #[default]
    Ui,
    /// Created explicitly by `sentinal watch`.
    Watch,
    /// Registered by an ordinary shell integration.
    ShellHook,
    /// Created by the opt-in interactive-shell wrapper.
    AutoWrap,
    /// Existing OS process registered read-only.
    Adopted,
}

impl std::fmt::Display for SessionOrigin {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            SessionOrigin::Ui => "UI",
            SessionOrigin::Watch => "WATCH",
            SessionOrigin::ShellHook => "SHELL_HOOK",
            SessionOrigin::AutoWrap => "AUTO_WRAP",
            SessionOrigin::Adopted => "ADOPTED",
        })
    }
}

/// How much structured and byte-stream state Sentinal can observe for a Session.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CaptureLevel {
    /// Daemon-owned PTY with output, input, Commands, and Metrics.
    #[default]
    Full,
    /// Structured Command boundaries and Metrics without terminal bytes.
    Commands,
    /// Read-only process log plus Metrics.
    ProcessLog,
    /// Process tree and Metrics only.
    Process,
}

impl std::fmt::Display for CaptureLevel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            CaptureLevel::Full => "FULL",
            CaptureLevel::Commands => "COMMANDS",
            CaptureLevel::ProcessLog => "PROCESS_LOG",
            CaptureLevel::Process => "PROCESS",
        })
    }
}

/// Operations and observations permitted by one capture level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureCapabilities {
    /// A client may attach to the terminal screen.
    pub can_attach: bool,
    /// A client may send terminal bytes.
    pub can_send_input: bool,
    /// A client may resize the daemon-owned PTY.
    pub can_resize: bool,
    /// A client may kill the daemon-owned PTY.
    pub can_kill_daemon_pty: bool,
    /// Structured Commands are observable.
    pub exposes_commands: bool,
    /// PTY output is observable.
    pub exposes_terminal_output: bool,
    /// A configured read-only log is observable.
    pub exposes_log_output: bool,
    /// Process resource Metrics are observable.
    pub exposes_metrics: bool,
}

impl CaptureLevel {
    /// Derive every interface capability from capture alone.
    pub const fn capabilities(self) -> CaptureCapabilities {
        match self {
            CaptureLevel::Full => CaptureCapabilities {
                can_attach: true,
                can_send_input: true,
                can_resize: true,
                can_kill_daemon_pty: true,
                exposes_commands: true,
                exposes_terminal_output: true,
                exposes_log_output: false,
                exposes_metrics: true,
            },
            CaptureLevel::Commands => CaptureCapabilities {
                can_attach: false,
                can_send_input: false,
                can_resize: false,
                can_kill_daemon_pty: false,
                exposes_commands: true,
                exposes_terminal_output: false,
                exposes_log_output: false,
                exposes_metrics: true,
            },
            CaptureLevel::ProcessLog => CaptureCapabilities {
                can_attach: false,
                can_send_input: false,
                can_resize: false,
                can_kill_daemon_pty: false,
                exposes_commands: false,
                exposes_terminal_output: false,
                exposes_log_output: true,
                exposes_metrics: true,
            },
            CaptureLevel::Process => CaptureCapabilities {
                can_attach: false,
                can_send_input: false,
                can_resize: false,
                can_kill_daemon_pty: false,
                exposes_commands: false,
                exposes_terminal_output: false,
                exposes_log_output: false,
                exposes_metrics: true,
            },
        }
    }
}

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
    /// How this Session entered Sentinal.
    #[serde(default)]
    pub origin: SessionOrigin,
    /// Observation and control capability for this Session.
    #[serde(default)]
    pub capture: CaptureLevel,
    /// OS process-tree root, or `None` when no trustworthy root is known.
    #[serde(default)]
    pub pid: Option<u32>,
    /// OS-observed root-process start time, used to detect PID reuse.
    #[serde(default)]
    pub process_started_at: Option<Timestamp>,
    /// Known shell exit code; absent means running or outcome unknown.
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// Log file of an adopted process, if one is known (not yet tailed in 1b.C).
    #[serde(default)]
    pub log: Option<SessionLog>,
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
            origin: SessionOrigin::Ui,
            capture: CaptureLevel::Full,
            pid: None,
            process_started_at: None,
            exit_code: None,
            log: None,
            created_at: at,
            updated_at: at,
        }
    }

    /// Validate capture, process identity, and lifecycle invariants.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        let valid_capture = matches!(
            (self.origin, self.capture),
            (SessionOrigin::Ui, CaptureLevel::Full)
                | (SessionOrigin::Watch, CaptureLevel::Full)
                | (SessionOrigin::AutoWrap, CaptureLevel::Full)
                | (SessionOrigin::ShellHook, CaptureLevel::Commands)
                | (SessionOrigin::Adopted, CaptureLevel::Process)
                | (SessionOrigin::Adopted, CaptureLevel::ProcessLog)
        );
        if !valid_capture {
            return Err(ProtocolError::InvalidOriginCapture {
                origin: self.origin,
                capture: self.capture,
            });
        }
        if self.pid == Some(0) {
            return Err(ProtocolError::InvalidPid(0));
        }
        if self.pid.is_none() && self.process_started_at.is_some() {
            return Err(ProtocolError::ProcessStartedWithoutPid);
        }
        if let Some(log) = &self.log {
            if self.origin != SessionOrigin::Adopted {
                return Err(ProtocolError::LogWithoutAdoption {
                    origin: self.origin,
                });
            }
            if !log.path.starts_with('/') {
                return Err(ProtocolError::RelativeLogPath(log.path.clone()));
            }
        }
        if let Some(exit_code) = self.exit_code {
            let valid_exit = matches!(
                (self.state, exit_code),
                (SessionState::Completed, 0) | (SessionState::Failed, 1..=i32::MAX)
            ) || matches!(self.state, SessionState::Failed) && exit_code < 0;
            if !valid_exit {
                return Err(ProtocolError::InvalidSessionExit {
                    state: self.state,
                    exit_code,
                });
            }
        }
        Ok(())
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

    #[test]
    fn every_capture_level_has_the_approved_capabilities() {
        let full = CaptureLevel::Full.capabilities();
        assert!(full.can_attach);
        assert!(full.can_send_input);
        assert!(full.can_resize);
        assert!(full.can_kill_daemon_pty);
        assert!(full.exposes_commands);
        assert!(full.exposes_terminal_output);
        assert!(!full.exposes_log_output);
        assert!(full.exposes_metrics);

        let commands = CaptureLevel::Commands.capabilities();
        assert!(!commands.can_attach);
        assert!(!commands.can_send_input);
        assert!(!commands.can_resize);
        assert!(!commands.can_kill_daemon_pty);
        assert!(commands.exposes_commands);
        assert!(!commands.exposes_terminal_output);
        assert!(!commands.exposes_log_output);
        assert!(commands.exposes_metrics);

        let process_log = CaptureLevel::ProcessLog.capabilities();
        assert!(!process_log.can_attach);
        assert!(!process_log.can_send_input);
        assert!(!process_log.can_resize);
        assert!(!process_log.can_kill_daemon_pty);
        assert!(!process_log.exposes_commands);
        assert!(!process_log.exposes_terminal_output);
        assert!(process_log.exposes_log_output);
        assert!(process_log.exposes_metrics);

        let process = CaptureLevel::Process.capabilities();
        assert!(!process.can_attach);
        assert!(!process.can_send_input);
        assert!(!process.can_resize);
        assert!(!process.can_kill_daemon_pty);
        assert!(!process.exposes_commands);
        assert!(!process.exposes_terminal_output);
        assert!(!process.exposes_log_output);
        assert!(process.exposes_metrics);
    }

    #[test]
    fn legacy_session_json_receives_safe_defaults() {
        let json = r#"{
            "id":1,"machine_id":"test","project_id":null,"title":null,
            "shell":"/bin/sh","cwd":"/work","state":"IDLE",
            "current_command":null,"created_at":1,"updated_at":2
        }"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.origin, SessionOrigin::Ui);
        assert_eq!(session.capture, CaptureLevel::Full);
        assert_eq!(session.pid, None);
        assert_eq!(session.process_started_at, None);
        assert_eq!(session.exit_code, None);
        assert_eq!(session.validate(), Ok(()));
    }

    #[test]
    fn invalid_origin_capture_and_process_identity_are_rejected() {
        let mut session = test_session();
        session.origin = SessionOrigin::ShellHook;
        assert!(session
            .validate()
            .unwrap_err()
            .to_string()
            .contains("SHELL_HOOK"));

        session.capture = CaptureLevel::Commands;
        session.pid = Some(0);
        assert!(session
            .validate()
            .unwrap_err()
            .to_string()
            .contains("pid 0"));

        session.pid = None;
        session.process_started_at = Some(Timestamp(10));
        assert!(session
            .validate()
            .unwrap_err()
            .to_string()
            .contains("process_started_at"));
    }

    #[test]
    fn log_metadata_is_only_valid_on_adopted_sessions_with_absolute_paths() {
        let mut session = test_session();
        session.log = Some(SessionLog {
            path: "/work/train.log".into(),
            source: crate::LogSource::Stdout,
        });
        assert!(session
            .validate()
            .unwrap_err()
            .to_string()
            .contains("ADOPTED"));

        session.origin = SessionOrigin::Adopted;
        session.capture = CaptureLevel::Process;
        assert_eq!(session.validate(), Ok(()));

        session.log = Some(SessionLog {
            path: "train.log".into(),
            source: crate::LogSource::Explicit,
        });
        assert!(session
            .validate()
            .unwrap_err()
            .to_string()
            .contains("absolute"));
    }
}
