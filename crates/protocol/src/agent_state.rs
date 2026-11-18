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
/// Most characters in a reported task name (6.5).
pub const MAX_AGENT_TASK_CHARS: usize = 80;
/// Most steps a plan may report (6.5).
pub const MAX_AGENT_STEPS: u32 = 10_000;
/// Longest ETA an agent may report: seven days (6.5).
pub const MAX_AGENT_ETA_SECS: u32 = 7 * 24 * 3600;

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
    /// The task the agent says it is on, e.g. `"migrate tests"` (6.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// The step the agent says it is on (6.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    /// How many steps the agent says its plan has (6.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<u32>,
    /// The agent's own guess of seconds left, as it reported it (6.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eta_secs: Option<u32>,
    /// When the daemon first saw the current `task`; set by the daemon only (6.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_started_at: Option<Timestamp>,
    /// Sentinal's own estimate of seconds left from the pace of recent
    /// steps; set by the daemon only, absent until two steps were seen (6.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_left_secs: Option<u32>,
    /// When the daemon accepted the report.
    pub reported_at: Timestamp,
}

impl AgentReportedState {
    /// A report with a status and nothing else.
    pub fn new(status: AgentStatus, reported_at: Timestamp) -> Self {
        Self {
            status,
            detail: None,
            task: None,
            step: None,
            steps: None,
            eta_secs: None,
            task_started_at: None,
            estimated_left_secs: None,
            reported_at,
        }
    }
}

/// Check reported progress (6.5): a task of at most 80 characters with no
/// control characters, `step <= steps <= 10 000`, and an ETA of at most 7 days.
pub fn validate_agent_progress(
    task: Option<&str>,
    step: Option<u32>,
    steps: Option<u32>,
    eta_secs: Option<u32>,
) -> Result<(), ProtocolError> {
    if let Some(task) = task {
        if task.trim().is_empty() {
            return Err(ProtocolError::InvalidAgentProgress("task is empty"));
        }
        if task.chars().count() > MAX_AGENT_TASK_CHARS {
            return Err(ProtocolError::InvalidAgentProgress(
                "task is longer than 80 characters",
            ));
        }
        if task.chars().any(char::is_control) {
            return Err(ProtocolError::InvalidAgentProgress(
                "task contains control characters",
            ));
        }
    }
    if step.is_some_and(|step| step > MAX_AGENT_STEPS)
        || steps.is_some_and(|steps| steps > MAX_AGENT_STEPS)
    {
        return Err(ProtocolError::InvalidAgentProgress(
            "steps are more than 10000",
        ));
    }
    if let (Some(step), Some(steps)) = (step, steps) {
        if step > steps {
            return Err(ProtocolError::InvalidAgentProgress(
                "step is past the last step",
            ));
        }
    }
    if eta_secs.is_some_and(|eta| eta > MAX_AGENT_ETA_SECS) {
        return Err(ProtocolError::InvalidAgentProgress(
            "eta is longer than 7 days",
        ));
    }
    Ok(())
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

/// Parse an ETA such as `90s`, `12m` or `2h` into seconds (6.5).
///
/// A unit is required; anything over 7 days, fractional or negative is `None`.
pub fn parse_agent_eta(text: &str) -> Option<u32> {
    let unit = text.chars().last()?;
    let number = &text[..text.len() - unit.len_utf8()];
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let scale = match unit {
        's' => 1,
        'm' => 60,
        'h' => 3600,
        _ => return None,
    };
    let secs = number.parse::<u64>().ok()?.checked_mul(scale)?;
    u32::try_from(secs)
        .ok()
        .filter(|secs| *secs <= MAX_AGENT_ETA_SECS)
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
