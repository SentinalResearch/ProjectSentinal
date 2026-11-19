//! An agent Session's conversation, read from the agent's own file (AC1).
//!
//! Design: `docs/AGENT_CHAT_DESIGN.md`. The daemon reads the conversation file
//! that the agent named through its hook and keeps only what is listed here:
//! the owner's messages, the agent's replies, its tool calls and short result
//! previews. Account IDs, reasoning and everything else in the file are
//! dropped. Chat entries go only where terminal output may go: never into the
//! control's dashboard projection, never into the store.

use crate::{ProtocolError, Timestamp};
use serde::{Deserialize, Serialize};

/// Most characters in a message's text.
pub const MAX_CHAT_TEXT_CHARS: usize = 8_000;
/// Most characters in a tool call's summary, e.g. `cargo test -p parser`.
pub const MAX_CHAT_SUMMARY_CHARS: usize = 200;
/// Most characters in a tool result's preview (decision 4: 4 KB).
pub const MAX_CHAT_PREVIEW_CHARS: usize = 4_096;
/// Most entries kept, and sent, per Session.
pub const MAX_CHAT_ENTRIES: usize = 500;
/// Longest conversation file path accepted.
pub const MAX_TRANSCRIPT_PATH_BYTES: usize = 4_096;

/// One thing in the conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChatItem {
    /// The owner's message.
    User {
        /// What was sent.
        text: String,
    },
    /// The agent's reply text.
    Agent {
        /// The reply.
        text: String,
    },
    /// The agent called a tool.
    Tool {
        /// The agent's id for this call, matching its result.
        id: String,
        /// The tool's name, e.g. `Bash`, `Edit`.
        tool: String,
        /// One line: the command, the file, or the query.
        summary: String,
    },
    /// A tool call's result.
    ToolResult {
        /// The call this answers.
        id: String,
        /// Whether the tool reported success.
        ok: bool,
        /// The first part of the result, secrets redacted.
        preview: String,
    },
    /// The agent finished a turn and waits for the owner.
    TurnFinished {
        /// Tool calls in that turn.
        tool_uses: u32,
    },
}

/// One timestamped chat item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ChatEntry {
    /// When the agent recorded it.
    pub at: Timestamp,
    /// What it was.
    pub item: ChatItem,
}

/// The first `max` characters of `text`, trimmed.
pub fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// A conversation file path an agent may name: absolute, a `.jsonl` file,
/// bounded, with no `..`. Where it lives and whose it is are checked by the
/// daemon against the real file system.
pub fn validate_transcript_path(path: &str) -> Result<(), ProtocolError> {
    let ok = path.starts_with('/')
        && path.len() <= MAX_TRANSCRIPT_PATH_BYTES
        && path.ends_with(".jsonl")
        && !path.split('/').any(|part| part == "..")
        && !path.chars().any(char::is_control);
    if ok {
        Ok(())
    } else {
        Err(ProtocolError::InvalidAgentChat {
            field: "path",
            reason: "must be an absolute .jsonl path",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_absolute_jsonl_paths_without_parent_steps_are_accepted() {
        assert!(validate_transcript_path("/Users/a/.claude/projects/x/1.jsonl").is_ok());
        for bad in [
            "relative/1.jsonl",
            "/Users/a/../b/1.jsonl",
            "/Users/a/1.json",
            "/Users/a/\n1.jsonl",
        ] {
            assert!(validate_transcript_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn clip_keeps_short_text_and_marks_cut_text() {
        assert_eq!(clip("  ok ", 10), "ok");
        assert_eq!(clip("abcdef", 4), "abc…");
    }
}
