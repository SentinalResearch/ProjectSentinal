#![allow(clippy::unwrap_used)]
//! Milestone 6.1: what an AI coding agent does, read from its screen.
//!
//! Wording is taken from the installed tools: Claude Code draws a tool call
//! as `⏺ Name(args)`, shows `esc to interrupt` while it works, prefixes API
//! failures with `API Error:`, and `/cost` prints `Total cost:` and
//! `Usage: N input, M output, …`. Gemini CLI marks a finished tool call with
//! `✓ Name args` and shows `esc to cancel` while it works. aider prints
//! `Applied edit to <file>` and `Tokens: … sent, … received. Cost: … session.`

use protocol::{
    coding_agent::{AgentAction, CodingAgent, CodingAgentSummary},
    Command, CommandId, EventKind, MachineId, SessionId, Timestamp,
};
use watcher::{
    agent_activity::{tool_line, usage_line, working_line, Usage},
    AdapterState, AgentCliAdapter, Observation, ObservationKind, WatcherAdapter,
};

fn start(adapter: &mut AgentCliAdapter, text: &str) {
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("m".into()),
        text,
        "/work",
        Timestamp(0),
    );
    adapter.observe(&Observation {
        at: Timestamp(0),
        kind: ObservationKind::CommandStarted(command),
    });
}

fn line(adapter: &mut AgentCliAdapter, at: i64, text: &str) -> Vec<EventKind> {
    adapter.observe(&Observation::output(Timestamp(at), text))
}

fn tick(adapter: &mut AgentCliAdapter, at: i64) -> Vec<EventKind> {
    adapter.observe(&Observation::tick(Timestamp(at)))
}

fn summary(adapter: &AgentCliAdapter) -> Option<CodingAgentSummary> {
    match adapter.state() {
        AdapterState::AgentCli(state) => state.summary,
        _ => None,
    }
}

#[test]
fn tool_lines_name_the_action_for_each_agent() {
    use AgentAction::*;
    let cases = [
        (
            "⏺ Bash(cargo build --release)",
            Some((RanCommand, "cargo build --release")),
        ),
        (
            "⏺ Bash(cargo test -p daemon)",
            Some((RanTests, "cargo test -p daemon")),
        ),
        (
            "● Bash(npx vitest run src/data)",
            Some((RanTests, "npx vitest run src/data")),
        ),
        (
            "⏺ Update(crates/daemon/src/api.rs)",
            Some((EditedFile, "crates/daemon/src/api.rs")),
        ),
        (
            "⏺ Write(docs/NOTES.md)",
            Some((EditedFile, "docs/NOTES.md")),
        ),
        ("⏺ Read(Cargo.toml)", Some((ReadFile, "Cargo.toml"))),
        (
            "⏺ Search(pattern: \"TODO\")",
            Some((Searched, "pattern: \"TODO\"")),
        ),
        (
            "⏺ Web Search(\"rust trigram\")",
            Some((Searched, "\"rust trigram\"")),
        ),
        (
            "⏺ TaskCreate(Fix the build)",
            Some((UsedTool, "Fix the build")),
        ),
        (
            " ✓  Shell cargo test --workspace",
            Some((RanTests, "cargo test --workspace")),
        ),
        (" ✓  ReadFile src/main.rs", Some((ReadFile, "src/main.rs"))),
        (" ✓  WriteFile src/lib.rs", Some((EditedFile, "src/lib.rs"))),
        (
            " ✓  SearchText TODO in src",
            Some((Searched, "TODO in src")),
        ),
        (
            "Applied edit to app/models.py",
            Some((EditedFile, "app/models.py")),
        ),
        // Prose, not a tool call.
        ("⏺ I'll fix the failing test (it was off by one).", None),
        ("⏺ Done.", None),
        ("✓ all good", None),
        ("plain output line", None),
    ];
    for (text, expected) in cases {
        let found = tool_line(text);
        assert_eq!(
            found
                .as_ref()
                .map(|(action, detail)| (*action, detail.as_str())),
            expected,
            "{text}"
        );
    }
}

#[test]
fn working_and_usage_lines_are_recognised() {
    assert!(working_line(
        "✻ Thinking… (12s · ↑ 1.2k tokens · esc to interrupt)"
    ));
    assert!(working_line("⠋ Reading files (esc to cancel, 4s)"));
    assert!(!working_line("> Try \"fix the bug\""));
    assert_eq!(
        usage_line("Total cost:            $0.4213"),
        Some(Usage {
            tokens: None,
            cost_usd: Some(0.4213)
        })
    );
    assert_eq!(
        usage_line(
            "Usage:                 12.5k input, 1.8k output, 340k cache read, 20k cache write"
        ),
        Some(Usage {
            tokens: Some(14_300),
            cost_usd: None
        })
    );
    assert_eq!(
        usage_line("Tokens: 2.4k sent, 312 received. Cost: $0.01 message, $0.04 session."),
        Some(Usage {
            tokens: Some(2_712),
            cost_usd: Some(0.04)
        })
    );
    assert_eq!(usage_line("Total cost is unknown"), None);
}

#[test]
fn a_claude_turn_becomes_activity_events_then_finished() {
    let mut adapter = AgentCliAdapter::new();
    start(&mut adapter, "claude");
    let mut events = Vec::new();
    // Ink redraws its live region: the same lines arrive more than once.
    for at in [100, 600, 1_100] {
        events.extend(line(&mut adapter, at, "✻ Working… (3s · esc to interrupt)"));
        events.extend(line(&mut adapter, at + 1, "⏺ Bash(cargo test -p daemon)"));
    }
    events.extend(line(
        &mut adapter,
        1_500,
        "⏺ Update(crates/daemon/src/api.rs)",
    ));
    events.extend(line(
        &mut adapter,
        1_600,
        "✻ Working… (5s · esc to interrupt)",
    ));
    events.extend(tick(&mut adapter, 2_000));
    assert_eq!(
        events,
        vec![
            EventKind::AgentActivity {
                action: AgentAction::RanTests,
                detail: "cargo test -p daemon".into(),
            },
            EventKind::AgentActivity {
                action: AgentAction::EditedFile,
                detail: "crates/daemon/src/api.rs".into(),
            },
        ]
    );
    assert!(summary(&adapter).unwrap().working);

    // Three quiet seconds without the working line: the turn is over.
    assert!(tick(&mut adapter, 4_000).is_empty());
    assert_eq!(
        tick(&mut adapter, 4_700),
        vec![EventKind::AgentTurnFinished { tool_uses: 2 }]
    );
    assert_eq!(
        summary(&adapter),
        Some(CodingAgentSummary {
            agent: CodingAgent::Claude,
            working: false,
            tool_uses: 2,
            tokens: None,
            cost_usd: None,
        })
    );
    // Only once.
    assert!(tick(&mut adapter, 9_000).is_empty());

    // The next turn counts its own tool calls; the Session keeps the total.
    line(&mut adapter, 10_000, "✻ Working… (1s · esc to interrupt)");
    line(&mut adapter, 10_100, "⏺ Bash(cargo test -p daemon)");
    assert_eq!(
        tick(&mut adapter, 14_000),
        vec![EventKind::AgentTurnFinished { tool_uses: 1 }]
    );
    assert_eq!(summary(&adapter).unwrap().tool_uses, 3);
}

#[test]
fn api_errors_are_errors_and_cost_lines_are_kept() {
    let mut adapter = AgentCliAdapter::new();
    start(&mut adapter, "npx @anthropic-ai/claude-code");
    assert_eq!(
        line(
            &mut adapter,
            100,
            "  ⎿  API Error: 529 {\"type\":\"overloaded_error\"}"
        ),
        vec![EventKind::ErrorDetected {
            message: "API Error: 529 {\"type\":\"overloaded_error\"}".into()
        }]
    );
    line(&mut adapter, 200, "Total cost:            $1.25");
    line(
        &mut adapter,
        201,
        "Usage:                 100k input, 20k output, 1m cache read, 0 cache write",
    );
    let summary = summary(&adapter).unwrap();
    assert_eq!(summary.cost_usd, Some(1.25));
    assert_eq!(summary.tokens, Some(120_000));
}

#[test]
fn every_agent_is_named_and_others_have_no_summary() {
    for (text, agent) in [
        ("claude --resume", CodingAgent::Claude),
        ("codex", CodingAgent::Codex),
        ("aider --model sonnet", CodingAgent::Aider),
        ("gemini", CodingAgent::Gemini),
    ] {
        let mut adapter = AgentCliAdapter::new();
        start(&mut adapter, text);
        assert_eq!(summary(&adapter).map(|s| s.agent), Some(agent), "{text}");
    }
    assert_eq!(summary(&AgentCliAdapter::new()), None);
}
