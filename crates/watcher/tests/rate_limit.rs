//! Progress Event rate-limit tests.

use protocol::{Command, CommandId, EventKind, MachineId, SessionId, Timestamp};
use watcher::{EventEngine, Observation};

fn training() -> Command {
    Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("machine".into()),
        "python train.py",
        "/work",
        Timestamp(0),
    )
}

#[test]
fn carriage_return_progress_is_rate_limited_unless_percent_moves_one_point() {
    let mut engine = EventEngine::new(Some(&training()), Vec::new());
    let first = engine.observe(&Observation::output(Timestamp(0), "1/100 step"));
    assert!(first
        .events
        .iter()
        .any(|event| matches!(event, EventKind::ProgressUpdated { .. })));
    let suppressed = engine.observe(&Observation::output(Timestamp(1_000), "1/100 step"));
    assert!(!suppressed
        .events
        .iter()
        .any(|event| matches!(event, EventKind::ProgressUpdated { .. })));
    let moved = engine.observe(&Observation::output(Timestamp(2_000), "2/100 step"));
    assert!(moved
        .events
        .iter()
        .any(|event| matches!(event, EventKind::ProgressUpdated { .. })));
    let elapsed = engine.observe(&Observation::output(Timestamp(7_000), "2/100 step"));
    assert!(elapsed
        .events
        .iter()
        .any(|event| matches!(event, EventKind::ProgressUpdated { .. })));
}
