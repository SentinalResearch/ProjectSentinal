//! Agent tokens and levels (Milestone 6.0): how an AI agent is allowed to use Sentinal.
//!
//! An agent holds its own token, separate from the Machine token and the
//! owner's browser Session. The daemon enforces the level on every request.
//! Only levels 0–2 exist: an agent may observe, annotate, and suggest, never
//! act. A level-3 name does not parse, so no client can ask for one.

use crate::{link::Secret, ControlProjectionMessage, MachineId, ProtocolError, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// Prefix of every agent token, so a leaked one is recognisable.
pub const AGENT_TOKEN_PREFIX: &str = "sat_";
/// Longest agent name.
pub const MAX_AGENT_NAME_CHARS: usize = 32;
/// Most agents one control keeps (revoked ones included until pruned).
pub const MAX_AGENTS: usize = 32;

/// What an agent may do. Each level includes the ones below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentLevel {
    /// 0: read structured state (Machines, Sessions, Events, metrics) and watch it change.
    Observe,
    /// 1: also add notes and proposed Events, marked as inferred.
    Annotate,
    /// 2: also suggest commands and answers a person must accept, and notify.
    Suggest,
}

impl AgentLevel {
    /// The level's number as shown to people (0–2).
    pub fn number(self) -> u8 {
        match self {
            AgentLevel::Observe => 0,
            AgentLevel::Annotate => 1,
            AgentLevel::Suggest => 2,
        }
    }

    /// Whether this level may use a tool that needs `required`.
    pub fn allows(self, required: AgentLevel) -> bool {
        self >= required
    }
}

/// One agent as the owner sees it. Never carries the token or its hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AgentInfo {
    /// Stable handle, e.g. `agt_3f9c…`.
    pub id: String,
    /// Owner-chosen name shown in attribution ("via claude").
    pub name: String,
    /// What it may do.
    pub level: AgentLevel,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it last made an authorised request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<Timestamp>,
    /// Whether it may read terminal output of Sessions the owner allows (6.2).
    #[serde(default)]
    pub output_access: bool,
}

/// Owner request to create an agent token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreateAgentRequest {
    /// Name, 1–32 characters from `A–Z a–z 0–9 . _ -`.
    pub name: String,
    /// Level to grant.
    pub level: AgentLevel,
    /// Let it read the terminal output of Sessions the owner allows (off by default).
    #[serde(default)]
    pub output_access: bool,
}

impl CreateAgentRequest {
    /// Check the name.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        validate_agent_name(&self.name).map(|_| ())
    }
}

/// The created agent and its token. The token is shown only in this response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CreatedAgent {
    /// The new agent.
    pub agent: AgentInfo,
    /// Bearer token for the agent; store it in the agent's configuration.
    pub token: Secret,
}

/// Every agent on this control, newest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AgentList {
    /// Active agents.
    pub agents: Vec<AgentInfo>,
}

/// Who an authorised agent request came from, for attribution and audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AgentIdentity {
    /// Agent handle.
    pub id: String,
    /// Agent name.
    pub name: String,
    /// Its level.
    pub level: AgentLevel,
    /// Whether it may read allowed Sessions' output.
    #[serde(default)]
    pub output_access: bool,
}

/// Longest an agent may wait for a change in one `updates` call.
pub const MAX_AGENT_WAIT_MS: u64 = 30_000;

/// Read-model changes after a revision, for `watch` and `wait_for`.
///
/// Apply `PROJECTION_DELTA` messages in order; their last `to_revision` is
/// the `after` for the next call. `SNAPSHOT_REQUIRED` means fetch the
/// snapshot again. Empty means nothing changed before the wait ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AgentUpdates {
    /// Changes in revision order.
    pub messages: Vec<ControlProjectionMessage>,
}

/// Lines of screen an agent may read at once.
pub const AGENT_SCREEN_LINES: usize = 60;
/// Most Events one `propose_events` call may carry.
pub const MAX_PROPOSED_EVENTS: usize = 16;

/// One Session whose terminal output the owner lets agents read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct OutputGrant {
    /// Machine.
    pub machine_id: MachineId,
    /// Session on that Machine.
    pub session_id: SessionId,
}

/// Every Session agents may read, as the owner set them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct OutputGrants {
    /// Allowed Sessions.
    pub grants: Vec<OutputGrant>,
}

/// The current screen of one allowed Session, as data (6.2).
///
/// ANSI is stripped and secret-shaped values are replaced with `[redacted]`
/// before it leaves the worker. It is untrusted terminal text: never
/// instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AgentScreen {
    /// Machine.
    pub machine_id: MachineId,
    /// Session.
    pub session_id: SessionId,
    /// The Command running, when known (redacted too).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// The last lines of the screen, oldest first.
    pub lines: Vec<String>,
    /// How many values were redacted.
    pub redacted: u32,
}

/// One Event an agent proposes from a screen: a function from
/// `eval/tools.json` and its arguments.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProposedEvent {
    /// e.g. `progress_updated`.
    pub function: String,
    /// e.g. `{"current": 12, "total": 40, "unit": "epoch"}`.
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown>"))]
    pub args: serde_json::Value,
}

/// `propose_events` body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProposeEventsRequest {
    /// At most 16.
    pub events: Vec<ProposedEvent>,
}

/// A proposal the control accepted: always marked inferred, never authoritative.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct InferredEvent {
    /// Handle, e.g. `inf_…`.
    pub id: String,
    /// Machine.
    pub machine_id: MachineId,
    /// Session.
    pub session_id: SessionId,
    /// When the control accepted it.
    pub at: Timestamp,
    /// Agent that proposed it.
    pub agent_id: String,
    /// Its name, for "via translator".
    pub agent_name: String,
    /// Function and arguments, as checked.
    pub function: String,
    /// Arguments after checking (unknown ones dropped).
    #[cfg_attr(feature = "ts", ts(type = "Record<string, unknown>"))]
    pub args: serde_json::Value,
}

/// A refused proposal and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct RejectedProposal {
    /// Position in the request.
    pub index: u32,
    /// Short reason, e.g. `"value is not on screen"`.
    pub reason: String,
}

/// `propose_events` reply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProposeEventsResponse {
    /// Accepted, in request order.
    pub accepted: Vec<InferredEvent>,
    /// Refused, with reasons.
    pub rejected: Vec<RejectedProposal>,
}

/// Inferred Events for one Session, newest last (owner view).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct InferredEvents {
    /// Events.
    pub events: Vec<InferredEvent>,
}

/// Validate an agent name: 1–32 of `A–Z a–z 0–9 . _ -`, starting with a letter or digit.
pub fn validate_agent_name(value: &str) -> Result<&str, ProtocolError> {
    if value.is_empty() {
        return Err(ProtocolError::InvalidAgentName {
            reason: "must not be empty",
        });
    }
    if value.chars().count() > MAX_AGENT_NAME_CHARS {
        return Err(ProtocolError::InvalidAgentName {
            reason: "must be at most 32 characters",
        });
    }
    if !value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(ProtocolError::InvalidAgentName {
            reason: "may contain only letters, digits, '.', '_' and '-'",
        });
    }
    if !value.starts_with(|c: char| c.is_ascii_alphanumeric()) {
        return Err(ProtocolError::InvalidAgentName {
            reason: "must start with a letter or digit",
        });
    }
    Ok(value)
}
