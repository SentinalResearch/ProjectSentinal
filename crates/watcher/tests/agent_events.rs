#![allow(clippy::unwrap_used)]
//! What a coding agent does reaches the Session's Events, through the engine.
//!
//! The adapter tests drive `AgentCliAdapter` directly; these drive
//! `EventEngine`, which also applies the capture filter and the screen.

use protocol::{CaptureLevel, Command, CommandId, EventKind, MachineId, SessionId, Timestamp};
use watcher::{AdapterState, EventEngine, Observation, ObservationKind};

fn claude() -> Command {
    Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("m".into()),
        "claude",
        "/work",
        Timestamp(0),
    )
}

/// An engine that has seen `claude` start, as the daemon drives it.
fn started(capture: CaptureLevel) -> EventEngine {
    let mut engine = EventEngine::new(None, Vec::new()).with_capture(capture);
    engine.observe(&Observation {
        at: Timestamp(0),
        kind: ObservationKind::CommandStarted(claude()),
    });
    engine
}

fn screen(at: i64, rows: &[&str]) -> Observation {
    Observation::screen(
        Timestamp(at),
        rows.iter().map(|row| (*row).to_string()).collect(),
        false,
    )
}

fn tool_uses(state: Option<AdapterState>) -> u32 {
    match state {
        Some(AdapterState::AgentCli(state)) => state.summary.map_or(0, |s| s.tool_uses),
        _ => 0,
    }
}

#[test]
fn agent_activity_and_turn_end_reach_the_events_of_a_full_session() {
    let mut engine = started(CaptureLevel::Full);
    let mut events = Vec::new();
    events.extend(
        engine
            .observe(&Observation::output(Timestamp(0), "⏺ Bash(cargo test)"))
            .events,
    );
    events.extend(
        engine
            .observe(&Observation::output(
                Timestamp(100),
                "✻ Working… (esc to interrupt)",
            ))
            .events,
    );
    events.extend(engine.observe(&Observation::tick(Timestamp(5_000))).events);

    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventKind::AgentActivity { .. })),
        "{events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventKind::AgentTurnFinished { tool_uses: 1 })),
        "{events:?}"
    );
}

#[test]
fn a_question_on_screen_does_not_end_the_turn() {
    let mut engine = started(CaptureLevel::Full);
    engine.observe(&screen(
        0,
        &["⏺ Bash(rm -rf build)", "✻ Working… (esc to interrupt)"],
    ));
    let dialog = [
        "⏺ Bash(rm -rf build)",
        "Do you want to proceed?",
        "❯ 1. Yes",
        "  2. No",
    ];
    let mut events = Vec::new();
    for at in (250..=10_000).step_by(250) {
        events.extend(engine.observe(&screen(at, &dialog)).events);
        events.extend(engine.observe(&Observation::tick(Timestamp(at))).events);
    }
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EventKind::WaitingForInput { .. })),
        "{events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventKind::AgentTurnFinished { .. })),
        "{events:?}"
    );
}

#[test]
fn a_tool_line_still_on_screen_after_the_turn_is_not_counted_again() {
    let mut engine = started(CaptureLevel::Full);
    engine.observe(&screen(
        0,
        &["⏺ Bash(cargo test)", "✻ Working… (esc to interrupt)"],
    ));
    // The turn ends; the tool line stays on screen.
    engine.observe(&screen(500, &["⏺ Bash(cargo test)", "> "]));
    engine.observe(&Observation::tick(Timestamp(5_000)));
    // The next turn starts with the old line still visible.
    engine.observe(&screen(
        6_000,
        &["⏺ Bash(cargo test)", "✻ Working… (esc to interrupt)"],
    ));
    engine.observe(&screen(
        6_250,
        &[
            "⏺ Bash(cargo test)",
            "⏺ Bash(cargo build)",
            "✻ Working… (esc to interrupt)",
        ],
    ));
    let update = engine.observe(&Observation::tick(Timestamp(12_000)));
    assert_eq!(tool_uses(update.adapter_state), 2);
}
