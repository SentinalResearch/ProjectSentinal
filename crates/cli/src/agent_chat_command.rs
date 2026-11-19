//! `sentinal agent-chat`: link Claude Code's conversation file to its Session (AC1).
//!
//! `link` runs from Claude Code's `SessionStart` hook. Claude Code passes the
//! hook a JSON object on stdin with `transcript_path`; inside a Sentinal
//! Session (`SENTINAL_SESSION_ID`) this names that file to the daemon, which
//! checks the caller runs inside the Session. Anywhere else it does nothing.
//! It always exits 0 and prints nothing, so it can never break Claude Code.
//!
//! `setup` shows the hook to add to `~/.claude/settings.json`, and adds it only
//! after the owner says yes (decision 7).

use crate::hook_report::{exchange, socket_path};
use protocol::{agent_chat::validate_transcript_path, LocalRequest, SessionId};
use serde_json::{json, Value};
use std::{
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

/// How long `link` waits for the daemon; Claude Code waits for the hook.
const LINK_DEADLINE: Duration = Duration::from_secs(2);
/// Most hook input read.
const MAX_HOOK_INPUT: u64 = 64 * 1024;
/// The command the hook runs.
pub const HOOK_COMMAND: &str = "sentinal agent-chat link";

/// The request for this hook input, or `None` outside a Session or without a path.
pub fn build_request(session_id: Option<&str>, hook_input: &str) -> Option<LocalRequest> {
    let id = session_id?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|id| *id > 0)?;
    let input: Value = serde_json::from_str(hook_input).ok()?;
    let path = input.get("transcript_path")?.as_str()?.to_string();
    validate_transcript_path(&path).ok()?;
    Some(LocalRequest::AgentTranscript {
        session_id: SessionId(id),
        path,
        caller_pid: None,
    })
}

/// `sentinal agent-chat link`.
pub async fn link(input: &mut dyn Read) -> ExitCode {
    let mut text = String::new();
    let _ = input.take(MAX_HOOK_INPUT).read_to_string(&mut text);
    let session_id = std::env::var("SENTINAL_SESSION_ID").ok();
    let Some(request) = build_request(session_id.as_deref(), &text) else {
        return ExitCode::SUCCESS;
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let explicit = std::env::var_os(protocol::local::SOCKET_ENV).map(PathBuf::from);
    if let Some(socket) = socket_path(explicit, home.as_deref(), runtime.as_deref()) {
        let _ = tokio::time::timeout(LINK_DEADLINE, exchange(&socket, request)).await;
    }
    ExitCode::SUCCESS
}

/// The hook entry Sentinal adds under `hooks.SessionStart`.
pub fn hook_entry() -> Value {
    json!({ "hooks": [{ "type": "command", "command": HOOK_COMMAND }] })
}

/// `settings` with the hook added once; `None` when it is already there.
pub fn with_hook(mut settings: Value) -> Option<Value> {
    if !settings.is_object() {
        settings = json!({});
    }
    let hooks = settings
        .as_object_mut()?
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        *hooks = json!({});
    }
    let start = hooks
        .as_object_mut()?
        .entry("SessionStart")
        .or_insert_with(|| json!([]));
    if !start.is_array() {
        *start = json!([]);
    }
    let present = start.as_array()?.iter().any(|entry| {
        entry["hooks"]
            .as_array()
            .is_some_and(|list| list.iter().any(|hook| hook["command"] == HOOK_COMMAND))
    });
    if present {
        return None;
    }
    start.as_array_mut()?.push(hook_entry());
    Some(settings)
}

/// `sentinal agent-chat setup [--print]`.
pub fn setup(
    print_only: bool,
    settings_path: &Path,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> ExitCode {
    let snippet = json!({ "hooks": { "SessionStart": [hook_entry()] } });
    let pretty = serde_json::to_string_pretty(&snippet).unwrap_or_default();
    if print_only {
        let _ = writeln!(out, "{pretty}");
        return ExitCode::SUCCESS;
    }
    let current = match std::fs::read_to_string(settings_path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(value) => value,
            Err(_) => {
                let _ = writeln!(
                    out,
                    "{} is not valid JSON; nothing was changed. Add this by hand:\n{pretty}",
                    settings_path.display()
                );
                return ExitCode::from(1);
            }
        },
        Err(_) => json!({}),
    };
    let Some(updated) = with_hook(current) else {
        let _ = writeln!(out, "The hook is already in {}.", settings_path.display());
        return ExitCode::SUCCESS;
    };
    let _ = writeln!(
        out,
        "This adds a SessionStart hook to {} so Sentinal can show Claude Code\n\
         Sessions as a chat. It only names the conversation file; Sentinal reads it\n\
         and stores nothing.\n\n{pretty}\n\nAdd it? [y/N] ",
        settings_path.display()
    );
    let mut answer = String::new();
    let _ = input.read_line(&mut answer);
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        let _ = writeln!(out, "Nothing was changed.");
        return ExitCode::SUCCESS;
    }
    let text = serde_json::to_string_pretty(&updated).unwrap_or_default() + "\n";
    if let Some(parent) = settings_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(settings_path, text) {
        Ok(()) => {
            let _ = writeln!(out, "Added. New Claude Code Sessions will show as a chat.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(out, "Could not write {}: {error}", settings_path.display());
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const HOOK_INPUT: &str = r#"{"session_id":"abc","transcript_path":"/Users/o/.claude/projects/-w/abc.jsonl","cwd":"/w","hook_event_name":"SessionStart"}"#;

    #[test]
    fn inside_a_session_it_names_the_file_and_not_the_caller() {
        let request = build_request(Some("7"), HOOK_INPUT).unwrap();
        assert_eq!(
            request,
            LocalRequest::AgentTranscript {
                session_id: SessionId(7),
                path: "/Users/o/.claude/projects/-w/abc.jsonl".into(),
                caller_pid: None,
            }
        );
    }

    #[test]
    fn outside_a_session_or_with_bad_input_it_sends_nothing() {
        assert!(build_request(None, HOOK_INPUT).is_none());
        assert!(build_request(Some("0"), HOOK_INPUT).is_none());
        assert!(build_request(Some("7"), "not json").is_none());
        assert!(build_request(Some("7"), r#"{"transcript_path":"relative.jsonl"}"#).is_none());
    }

    #[test]
    fn the_hook_is_added_once_and_other_settings_are_kept() {
        let settings = json!({"model": "opus", "hooks": {"Stop": [{"hooks": []}]}});
        let added = with_hook(settings).unwrap();
        assert_eq!(added["model"], "opus");
        assert!(added["hooks"]["Stop"].is_array());
        assert_eq!(added["hooks"]["SessionStart"][0], hook_entry());
        assert!(with_hook(added).is_none(), "already there");
    }

    #[test]
    fn setup_changes_nothing_without_a_yes() {
        let dir = std::env::temp_dir().join(format!("sentinal-chat-setup-{}", std::process::id()));
        let path = dir.join("settings.json");
        let mut out = Vec::new();
        setup(false, &path, &mut "n\n".as_bytes(), &mut out);
        assert!(!path.exists());
        setup(false, &path, &mut "y\n".as_bytes(), &mut out);
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["hooks"]["SessionStart"][0], hook_entry());
        let _ = std::fs::remove_dir_all(dir);
    }
}
