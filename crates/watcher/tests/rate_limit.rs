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

fn kinds(update: &watcher::EngineUpdate) -> Vec<(&'static str, String)> {
    update
        .events
        .iter()
        .filter_map(|event| match event {
            EventKind::WarningDetected { message } => Some(("warning", message.clone())),
            EventKind::ErrorDetected { message } => Some(("error", message.clone())),
            _ => None,
        })
        .collect()
}

#[test]
fn a_burst_of_warnings_keeps_the_first_five_then_reports_a_count() {
    let mut engine = EventEngine::new(None, Vec::new());
    let mut kept = 0;
    for line in 0..20 {
        let update = engine.observe(&Observation::output(
            Timestamp(line * 100),
            format!("warning: unused variable `v{line}`"),
        ));
        kept += kinds(&update).len();
        if line >= 5 {
            assert!(
                kinds(&update).is_empty(),
                "line {line} is past the first five"
            );
        }
    }
    assert_eq!(kept, 5);
    // Errors are a burst of their own, even in the middle of the warnings.
    let error = engine.observe(&Observation::output(
        Timestamp(1_950),
        "error: could not compile",
    ));
    assert_eq!(kinds(&error).len(), 1);
    assert_eq!(kinds(&error)[0].0, "error");

    let summary = engine.observe(&Observation {
        at: Timestamp(10_000),
        kind: watcher::ObservationKind::Tick,
    });
    assert_eq!(
        kinds(&summary),
        vec![("warning", "15 more warnings in 10 s".to_string())]
    );

    let next = engine.observe(&Observation::output(
        Timestamp(10_500),
        "warning: deprecated",
    ));
    assert_eq!(kinds(&next).len(), 1, "a new burst starts after the window");
}

#[test]
fn a_lone_warning_after_the_window_has_no_count() {
    let mut engine = EventEngine::new(None, Vec::new());
    engine.observe(&Observation::output(Timestamp(0), "warning: one"));
    let later = engine.observe(&Observation::output(Timestamp(20_000), "warning: two"));
    assert_eq!(kinds(&later).len(), 1);
    assert!(!kinds(&later)[0].1.contains("more"));
}
