//! Agent tokens and levels (Milestone 6.0): how an AI agent is allowed to use Sentinal.
//!
//! An agent holds its own token, separate from the Machine token and the
//! owner's browser Session. The daemon enforces the level on every request.
//! Only levels 0–2 exist: an agent may observe, annotate, and suggest, never
//! act. A level-3 name does not parse, so no client can ask for one.

use crate::{link::Secret, ControlProjectionMessage, ProtocolError, Timestamp};
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
}

impl AgentIdentity {
    /// "via claude (level 2)": how audit and the UI name the agent's work.
    pub fn attribution(&self) -> String {
        format!("via {} (level {})", self.name, self.level.number())
    }
}

/// Calls kept per agent for its activity page (6.0 attribution).
pub const MAX_AGENT_ACTIVITY: usize = 100;

/// Whether the control served an agent's call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentCallOutcome {
    /// Served.
    Allowed,
    /// Refused: the tool needs a higher level.
    Denied,
}

/// One call an agent made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AgentCall {
    /// When.
    pub at: Timestamp,
    /// The tool (route), e.g. `snapshot`.
    pub tool: String,
    /// Served or refused.
    pub outcome: AgentCallOutcome,
}

/// What one agent has done, for Settings → Agents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AgentActivity {
    /// The agent.
    pub agent: AgentInfo,
    /// "via claude (level 2)".
    pub via: String,
    /// Calls since the control started.
    pub total_calls: u64,
    /// The latest calls, newest first, at most [`MAX_AGENT_ACTIVITY`].
    pub calls: Vec<AgentCall>,
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
