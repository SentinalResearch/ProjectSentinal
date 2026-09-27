//! A command: one line the user ran in a session, turned into structured data.
//!
//! Commands are detected by shell hooks (OSC 133), not by guessing from output.

use crate::{CommandId, DurationMs, MachineId, ProtocolError, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// What kind of work a command is. Filled in later by rules or a small model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CommandClass {
    /// Model training, e.g. `python train.py`.
    MlTraining,
    /// Running tests, e.g. `pytest`, `cargo test`.
    Test,
    /// Building, e.g. `cargo build`, `npm run build`.
    Build,
    /// A long-running server, e.g. `vllm serve`, `uvicorn`.
    Server,
    /// Installing packages, e.g. `pip install`.
    PackageInstall,
    /// Version control, e.g. `git push`.
    Git,
    /// Short everyday shell commands, e.g. `ls`, `cd`.
    Shell,
    /// Anything else.
    Other,
}

/// Derived status of a command. Not sent on the wire; computed from `exit_code`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    /// Still running.
    Running,
    /// Finished with exit code 0.
    Succeeded,
    /// Finished with a non-zero exit code.
    Failed,
}

/// One command run in a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Command {
    /// ID, unique on this machine.
    pub id: CommandId,
    /// Session it ran in.
    pub session_id: SessionId,
    /// Machine it ran on.
    pub machine_id: MachineId,
    /// The exact text the user ran.
    pub text: String,
    /// Working directory when it started.
    pub cwd: String,
    /// When it started.
    pub started_at: Timestamp,
    /// When it finished; `None` while running.
    pub finished_at: Option<Timestamp>,
    /// Exit code; `None` while running.
    pub exit_code: Option<i32>,
    /// What kind of work this is, if classified.
    pub class: Option<CommandClass>,
}

impl Command {
    /// A new, running command.
    pub fn start(
        id: CommandId,
        session_id: SessionId,
        machine_id: MachineId,
        text: impl Into<String>,
        cwd: impl Into<String>,
        at: Timestamp,
    ) -> Self {
        Command {
            id,
            session_id,
            machine_id,
            text: text.into(),
            cwd: cwd.into(),
            started_at: at,
            finished_at: None,
            exit_code: None,
            class: None,
        }
    }

    /// Mark the command finished. Fails if it already finished or `at` is before the start.
    pub fn finish(&mut self, exit_code: i32, at: Timestamp) -> Result<(), ProtocolError> {
        if self.exit_code.is_some() {
            return Err(ProtocolError::CommandAlreadyFinished(self.id));
        }
        if at < self.started_at {
            return Err(ProtocolError::FinishedBeforeStart {
                started_at: self.started_at.0,
                finished_at: at.0,
            });
        }
        self.exit_code = Some(exit_code);
        self.finished_at = Some(at);
        Ok(())
    }

    /// Running, succeeded or failed.
    pub fn status(&self) -> CommandStatus {
        match self.exit_code {
            None => CommandStatus::Running,
            Some(0) => CommandStatus::Succeeded,
            Some(_) => CommandStatus::Failed,
        }
    }

    /// How long it ran. For a running command, measured up to `now`.
    pub fn duration(&self, now: Timestamp) -> DurationMs {
        self.finished_at.unwrap_or(now).since(self.started_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_command() -> Command {
        Command::start(
            CommandId(1),
            SessionId(1),
            "rtx-pc".into(),
            "pytest tests/",
            "~/proj",
            Timestamp(1_000),
        )
    }

    #[test]
    fn new_command_is_running() {
        assert_eq!(test_command().status(), CommandStatus::Running);
    }

    #[test]
    fn exit_code_zero_means_succeeded() {
        let mut c = test_command();
        c.finish(0, Timestamp(2_000)).unwrap();
        assert_eq!(c.status(), CommandStatus::Succeeded);
    }

    #[test]
    fn non_zero_exit_code_means_failed() {
        for code in [1, 2, 127, 130, -1] {
            let mut c = test_command();
            c.finish(code, Timestamp(2_000)).unwrap();
            assert_eq!(c.status(), CommandStatus::Failed, "exit code {code}");
        }
    }

    #[test]
    fn finishing_twice_is_an_error() {
        let mut c = test_command();
        c.finish(0, Timestamp(2_000)).unwrap();
        assert_eq!(
            c.finish(1, Timestamp(3_000)),
            Err(ProtocolError::CommandAlreadyFinished(CommandId(1)))
        );
        assert_eq!(c.exit_code, Some(0), "first result must be kept");
    }

    #[test]
    fn finishing_before_start_is_an_error() {
        let mut c = test_command();
        assert!(matches!(
            c.finish(0, Timestamp(999)),
            Err(ProtocolError::FinishedBeforeStart { .. })
        ));
        assert_eq!(c.status(), CommandStatus::Running);
    }

    #[test]
    fn finishing_at_the_start_instant_is_allowed() {
        let mut c = test_command();
        c.finish(0, Timestamp(1_000)).unwrap();
        assert_eq!(c.duration(Timestamp(5_000)), DurationMs(0));
    }

    #[test]
    fn running_command_duration_is_measured_to_now() {
        assert_eq!(
            test_command().duration(Timestamp(43_000)),
            DurationMs(42_000)
        );
    }

    #[test]
    fn finished_command_duration_ignores_now() {
        let mut c = test_command();
        c.finish(1, Timestamp(43_000)).unwrap();
        assert_eq!(c.duration(Timestamp(999_999)), DurationMs(42_000));
    }
}
