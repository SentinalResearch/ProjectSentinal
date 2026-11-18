//! What an AI agent running inside a Session says it is doing (Milestone 6.4).
//!
//! The agent reports over the private local socket; the daemon checks that the
//! caller runs inside that Session before accepting it. A report is
//! presentation only: it never changes the Session's lifecycle state, never
//! runs anything, and never notifies on its own.

use crate::{ProtocolError, Timestamp};
use serde::{Deserialize, Serialize};

/// Most characters in a reported detail line.
pub const MAX_AGENT_DETAIL_CHARS: usize = 200;

/// What the agent says it is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentStatus {
    /// Working on the task.
    Working,
    /// Cannot go on without something from the owner.
    Blocked,
    /// Waiting for an answer or an outside event.
    Waiting,
    /// Finished what it was asked to do.
    Done,
}

/// The latest state an agent reported for its own Session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AgentReportedState {
    /// What the agent says it is doing.
    pub status: AgentStatus,
    /// One owner-visible line, e.g. `"needs approval for cargo test"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// When the daemon accepted the report.
    pub reported_at: Timestamp,
}

/// Check a reported detail: non-empty, at most 200 characters, no control characters.
pub fn validate_agent_detail(detail: Option<&str>) -> Result<(), ProtocolError> {
    let Some(detail) = detail else {
        return Ok(());
    };
    if detail.trim().is_empty() {
        return Err(ProtocolError::InvalidAgentDetail("is empty"));
    }
    if detail.chars().count() > MAX_AGENT_DETAIL_CHARS {
        return Err(ProtocolError::InvalidAgentDetail(
            "is longer than 200 characters",
        ));
    }
    if detail.chars().any(char::is_control) {
        return Err(ProtocolError::InvalidAgentDetail(
            "contains control characters",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_detail_is_accepted_and_none_is_fine() {
        assert_eq!(
            validate_agent_detail(Some("needs approval for cargo test")),
            Ok(())
        );
        assert_eq!(validate_agent_detail(None), Ok(()));
    }

    #[test]
    fn empty_long_or_control_details_are_refused() {
        assert!(validate_agent_detail(Some("  ")).is_err());
        assert!(validate_agent_detail(Some(&"a".repeat(201))).is_err());
        assert_eq!(validate_agent_detail(Some(&"é".repeat(200))), Ok(()));
        assert!(validate_agent_detail(Some("done\x1b[31m")).is_err());
        assert!(validate_agent_detail(Some("two\nlines")).is_err());
    }

    #[test]
    fn status_is_screaming_snake_case_on_the_wire() {
        let json = serde_json::to_string(&AgentStatus::Blocked).unwrap();
        assert_eq!(json, "\"BLOCKED\"");
    }
}
