//! AI coding agents running in a Session (Milestone 6.1): which agent, what
//! it did, and what it used. Closed codes and numbers only, so the summary
//! may travel to the control; tool arguments stay in Events on the Machine.

use serde::{Deserialize, Serialize};

/// Longest tool argument kept in an activity Event, in characters.
pub const MAX_AGENT_DETAIL_CHARS: usize = 200;

/// An AI coding agent Sentinal recognises from its Command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CodingAgent {
    /// Claude Code.
    Claude,
    /// OpenAI Codex CLI.
    Codex,
    /// aider.
    Aider,
    /// Gemini CLI.
    Gemini,
}

impl CodingAgent {
    /// Display name, e.g. "Claude Code".
    pub fn display_name(self) -> &'static str {
        match self {
            CodingAgent::Claude => "Claude Code",
            CodingAgent::Codex => "Codex",
            CodingAgent::Aider => "aider",
            CodingAgent::Gemini => "Gemini CLI",
        }
    }
}

/// What a tool call did, in closed terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentAction {
    /// Ran a shell command.
    RanCommand,
    /// Ran a test command.
    RanTests,
    /// Wrote or edited a file.
    EditedFile,
    /// Read a file or folder.
    ReadFile,
    /// Searched files or the web.
    Searched,
    /// Any other tool.
    UsedTool,
}

/// What Sentinal knows about the agent in a Session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct CodingAgentSummary {
    /// Which agent.
    pub agent: CodingAgent,
    /// Working on a turn now (its "esc to interrupt" line is showing).
    pub working: bool,
    /// Tool calls seen in this Command.
    pub tool_uses: u32,
    /// Tokens the agent last reported, when it prints them.
    pub tokens: Option<u64>,
    /// Cost in US dollars the agent last reported, when it prints it.
    pub cost_usd: Option<f64>,
}
