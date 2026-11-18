//! `sentinal agent-state`: an agent inside a Session tells the owner what it
//! is doing (Milestone 6.4).
//!
//! It sends one `AGENT_STATE` over the private local socket within 50 ms and
//! prints nothing. Outside a Session, or when the daemon is down, it exits 0
//! quietly, like the shell hooks, so an agent can always call it.

use crate::hook_report::{exchange, socket_path, REPORT_DEADLINE};
use protocol::{agent_state::validate_agent_detail, AgentStatus, LocalRequest, SessionId};
use std::{io::Write, path::PathBuf, process::ExitCode};

/// The states an agent may report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum StatusArg {
    /// Making progress on the task.
    Working,
    /// Cannot go on without the owner.
    Blocked,
    /// Waiting on something outside.
    Waiting,
    /// Finished.
    Done,
}

impl From<StatusArg> for AgentStatus {
    fn from(status: StatusArg) -> Self {
        match status {
            StatusArg::Working => AgentStatus::Working,
            StatusArg::Blocked => AgentStatus::Blocked,
            StatusArg::Waiting => AgentStatus::Waiting,
            StatusArg::Done => AgentStatus::Done,
        }
    }
}

/// The request to send, `Ok(None)` when not inside a Session, or the reason
/// the arguments are invalid.
pub fn build_request(
    session_id: Option<&str>,
    status: StatusArg,
    detail: Option<String>,
) -> Result<Option<LocalRequest>, String> {
    validate_agent_detail(detail.as_deref()).map_err(|error| error.to_string())?;
    let Some(id) = session_id
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|id| *id > 0)
    else {
        return Ok(None);
    };
    Ok(Some(LocalRequest::AgentState {
        session_id: SessionId(id),
        status: status.into(),
        detail,
        caller_pid: None,
    }))
}

/// Run `sentinal agent-state`; never writes to stdout.
pub async fn run(status: StatusArg, detail: Option<String>, err: &mut dyn Write) -> ExitCode {
    let session_id = std::env::var("SENTINAL_SESSION_ID").ok();
    let request = match build_request(session_id.as_deref(), status, detail) {
        Ok(Some(request)) => request,
        Ok(None) => return ExitCode::SUCCESS,
        Err(reason) => {
            let _ = writeln!(err, "sentinal agent-state: {reason}");
            return ExitCode::from(2);
        }
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let explicit = std::env::var_os("SENTINAL_SOCKET").map(PathBuf::from);
    let Some(socket) = socket_path(explicit, home.as_deref(), runtime.as_deref()) else {
        return ExitCode::SUCCESS;
    };
    // Silent on every delivery failure: a report must never disturb the agent.
    let _ = tokio::time::timeout(REPORT_DEADLINE, exchange(&socket, request)).await;
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_a_session_it_builds_a_report_without_naming_the_caller() {
        let request = build_request(Some("7"), StatusArg::Blocked, Some("needs the key".into()))
            .unwrap()
            .unwrap();
        assert_eq!(
            request,
            LocalRequest::AgentState {
                session_id: SessionId(7),
                status: AgentStatus::Blocked,
                detail: Some("needs the key".into()),
                caller_pid: None,
            }
        );
    }

    #[test]
    fn outside_a_session_it_does_nothing() {
        for value in [None, Some(""), Some("abc"), Some("0")] {
            assert_eq!(build_request(value, StatusArg::Working, None), Ok(None));
        }
    }

    #[test]
    fn a_bad_detail_is_an_error_even_outside_a_session() {
        assert!(build_request(None, StatusArg::Done, Some("bell\x07".into())).is_err());
        assert!(build_request(Some("7"), StatusArg::Done, Some("x".repeat(201))).is_err());
    }
}
