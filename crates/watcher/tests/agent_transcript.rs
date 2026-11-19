#![allow(clippy::unwrap_used)]
//! Claude Code's conversation file as chat and progress (AC1). The lines are
//! the shapes Claude Code 2.1.328 wrote in `just dev` on 2026-11-19, scrubbed.

use protocol::{ChatItem, Timestamp};
use watcher::agent_transcript::{parse_rfc3339, TranscriptReader};

const ACCOUNT: &str = "acct-0000-secret-id";

fn transcript() -> Vec<String> {
    let at = |s: u32| format!("2026-11-19T10:26:{s:02}.000Z");
    vec![
        format!(r#"{{"type":"permission-mode","permissionMode":"default","sessionId":"s1"}}"#),
        format!(
            r#"{{"type":"user","timestamp":"{}","accountUuid":"{ACCOUNT}","sessionId":"s1","message":{{"role":"user","content":"Create three files a.txt, b.txt and c.txt, one task each."}}}}"#,
            at(0)
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"thinking","thinking":"private reasoning"}},{{"type":"text","text":"I'll plan this first."}},{{"type":"tool_use","id":"t1","name":"TaskCreate","input":{{"subject":"touch a.txt","description":"x"}}}}]}}}}"#,
            at(1)
        ),
        format!(
            r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t1","is_error":false,"content":"Task #1 created successfully: touch a.txt"}}]}}}}"#,
            at(2)
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t2","name":"TaskCreate","input":{{"subject":"touch b.txt","description":"x"}}}}]}}}}"#,
            at(3)
        ),
        format!(
            r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t2","is_error":false,"content":"Task #2 created successfully: touch b.txt"}}]}}}}"#,
            at(4)
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t3","name":"TaskUpdate","input":{{"taskId":"1","status":"in_progress"}}}}]}}}}"#,
            at(5)
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t4","name":"Bash","input":{{"command":"touch a.txt","description":"Create a.txt"}}}}]}}}}"#,
            at(6)
        ),
        format!(
            r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t4","is_error":false,"content":[{{"type":"text","text":"(Bash completed with no output)"}}]}}]}}}}"#,
            at(7)
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t5","name":"TaskUpdate","input":{{"taskId":"1","status":"completed"}}}},{{"type":"tool_use","id":"t6","name":"TaskUpdate","input":{{"taskId":"2","status":"in_progress"}}}}]}}}}"#,
            at(8)
        ),
        format!(
            r#"{{"type":"user","timestamp":"{}","isMeta":true,"message":{{"role":"user","content":"meta note"}}}}"#,
            at(9)
        ),
        format!(
            r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":"<command-name>/model</command-name>"}}}}"#,
            at(10)
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"role":"assistant","stop_reason":"tool_use","content":[{{"type":"tool_use","id":"t7","name":"Bash","input":{{"command":"touch b.txt && curl -H 'Authorization: Bearer sk-ant-abcdefghijklmnopqrstuvwxyz0123456789' x"}}}}]}}}}"#,
            at(11)
        ),
        format!(
            r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":[{{"type":"tool_result","tool_use_id":"t7","is_error":true,"content":"curl: (6) Could not resolve host: x"}}]}}}}"#,
            at(12)
        ),
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"role":"assistant","stop_reason":"end_turn","content":[{{"type":"text","text":"a.txt and b.txt exist; the curl failed."}}]}}}}"#,
            at(13)
        ),
    ]
}

#[test]
fn a_conversation_becomes_chat_entries_in_order() {
    let mut reader = TranscriptReader::default();
    let entries: Vec<ChatItem> = transcript()
        .iter()
        .flat_map(|line| reader.line(line).entries)
        .map(|entry| entry.item)
        .collect();
    let kinds: Vec<&str> = entries
        .iter()
        .map(|item| match item {
            ChatItem::User { .. } => "user",
            ChatItem::Agent { .. } => "agent",
            ChatItem::Tool { .. } => "tool",
            ChatItem::ToolResult { .. } => "result",
            ChatItem::TurnFinished { .. } => "finished",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "user", "agent", "tool", "result", "tool", "result", "tool", "tool", "result", "tool",
            "tool", "tool", "result", "agent", "finished"
        ]
    );
    assert!(entries.contains(&ChatItem::Tool {
        id: "t4".into(),
        tool: "Bash".into(),
        summary: "touch a.txt".into()
    }));
    assert!(entries.contains(&ChatItem::ToolResult {
        id: "t7".into(),
        ok: false,
        preview: "curl: (6) Could not resolve host: x".into()
    }));
    // Plan calls are not counted as work.
    assert_eq!(
        entries.last(),
        Some(&ChatItem::TurnFinished { tool_uses: 2 })
    );
}

#[test]
fn nothing_private_leaves_the_reader() {
    let mut reader = TranscriptReader::default();
    let all: String = transcript()
        .iter()
        .flat_map(|line| reader.line(line).entries)
        .map(|entry| format!("{:?}", entry.item))
        .collect();
    assert!(!all.contains(ACCOUNT), "account id leaked");
    assert!(!all.contains("private reasoning"), "reasoning leaked");
    assert!(!all.contains("meta note") && !all.contains("/model"));
    assert!(
        !all.contains("abcdefghijklmnopqrstuvwxyz0123456789"),
        "token leaked: {all}"
    );
}

#[test]
fn the_agent_s_plan_gives_step_n_of_m_and_what_it_is_doing() {
    let mut reader = TranscriptReader::default();
    let lines = transcript();
    for line in &lines[..8] {
        reader.line(line);
    }
    // Task 1 in progress, Bash running.
    let progress = reader.progress();
    assert!(progress.working);
    assert_eq!(progress.task.as_deref(), Some("touch a.txt"));
    assert_eq!((progress.step, progress.steps), (Some(1), Some(2)));
    assert_eq!(progress.detail.as_deref(), Some("Running touch a.txt"));

    reader.line(&lines[8]);
    reader.line(&lines[9]);
    let progress = reader.progress();
    assert_eq!(progress.task.as_deref(), Some("touch b.txt"));
    assert_eq!(progress.step, Some(2));

    for line in &lines[10..] {
        reader.line(line);
    }
    let progress = reader.progress();
    assert!(!progress.working, "end_turn ends the turn");
    assert_eq!(progress.detail, None);
}

#[test]
fn without_a_plan_the_task_is_what_the_owner_asked() {
    let mut reader = TranscriptReader::default();
    reader.line(r#"{"type":"user","timestamp":"2026-11-19T10:00:00.000Z","message":{"role":"user","content":"Fix the failing parser test\nand run it"}}"#);
    let progress = reader.progress();
    assert_eq!(
        progress.task.as_deref(),
        Some("Fix the failing parser test")
    );
    assert_eq!((progress.step, progress.steps), (None, None));
}

#[test]
fn unreadable_or_unknown_lines_are_skipped_and_counted() {
    let mut reader = TranscriptReader::default();
    assert!(reader.line("not json").entries.is_empty());
    assert!(reader
        .line(r#"{"type":"new-kind","x":1}"#)
        .entries
        .is_empty());
    assert!(reader.line(&"x".repeat((1 << 20) + 1)).entries.is_empty());
    assert_eq!(reader.unreadable(), (2, 3));
}

#[test]
fn claude_code_timestamps_are_utc_milliseconds() {
    assert_eq!(
        parse_rfc3339("1970-01-01T00:00:01.500Z"),
        Some(Timestamp(1_500))
    );
    assert_eq!(
        parse_rfc3339("2026-11-19T10:26:15.123Z"),
        Some(Timestamp(1_795_083_975_123))
    );
    assert_eq!(parse_rfc3339("2026-11-19T10:26:15"), None);
}
