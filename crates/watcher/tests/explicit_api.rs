#![allow(clippy::unwrap_used)]
//! The `::sentinal` printed-line API: once a Session prints one explicit line,
//! it is trusted over adapter and fallback guesses (B2).

use protocol::{CaptureLevel, Command, CommandId, EventKind, MachineId, SessionId, Timestamp};
use watcher::{EventEngine, Observation};

fn command(text: &str) -> Command {
    Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("machine".into()),
        text,
        "/work",
        Timestamp(0),
    )
}

fn engine(command_text: &str) -> EventEngine {
    EventEngine::new(Some(&command(command_text)), Vec::new())
        .with_capture(CaptureLevel::ProcessLog)
}

fn events_at(engine: &mut EventEngine, line: &str, at_ms: i64) -> Vec<EventKind> {
    engine
        .observe(&Observation::output(Timestamp(at_ms), line))
        .events
}

#[test]
fn explicit_progress_line_produces_progress_event() {
    let mut engine = engine("python train.py");
    let events = events_at(
        &mut engine,
        "::sentinal progress current=3 total=50 unit=task",
        0,
    );
    assert!(events.contains(&EventKind::ProgressUpdated {
        current: 3,
        total: Some(50),
        unit: "task".into(),
    }));
}

#[test]
fn explicit_metric_line_produces_metric_reported_event() {
    let mut engine = engine("python train.py");
    let events = events_at(&mut engine, "::sentinal metric name=loss value=0.5", 0);
    assert!(events.contains(&EventKind::MetricReported {
        name: "loss".into(),
        value: 0.5,
    }));
}

#[test]
fn after_explicit_line_fallback_counters_are_ignored() {
    let mut engine = engine("python gsm8k_pilot.py");
    // First the explicit API line: sets explicit_api = true.
    events_at(
        &mut engine,
        "::sentinal progress current=1 total=10 unit=step",
        0,
    );
    // Now a fallback counter that would normally produce progress:
    let events = events_at(&mut engine, "task 5/20", 10_000);
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, EventKind::ProgressUpdated { .. })),
        "fallback counter must not override explicit API"
    );
}

#[test]
fn explicit_progress_without_total_is_allowed() {
    let mut engine = engine("python script.py");
    let events = events_at(&mut engine, "::sentinal progress current=42", 0);
    assert!(events.contains(&EventKind::ProgressUpdated {
        current: 42,
        total: None,
        unit: "item".into(),
    }));
}

#[test]
fn malformed_explicit_lines_are_shown_as_normal_output() {
    let mut engine = engine("python script.py");
    // current > total is invalid; no event, no explicit_api set.
    let events = events_at(&mut engine, "::sentinal progress current=9 total=5", 0);
    assert!(events.is_empty());
    // Fallback still works afterwards.
    let events = events_at(&mut engine, "step 3/10", 10_000);
    assert!(events.contains(&EventKind::ProgressUpdated {
        current: 3,
        total: Some(10),
        unit: "step".into(),
    }));
}

#[test]
fn explicit_api_resets_on_new_command() {
    let mut engine = engine("python first.py");
    events_at(&mut engine, "::sentinal progress current=1 total=10", 0);
    // New command starts: explicit_api resets.
    engine.observe(&Observation {
        at: Timestamp(1000),
        kind: watcher::ObservationKind::CommandStarted(command("python second.py")),
    });
    let events = events_at(&mut engine, "task 7/30", 20_000);
    assert!(
        events.contains(&EventKind::ProgressUpdated {
            current: 7,
            total: Some(30),
            unit: "task".into(),
        }),
        "fallback must work again after a new Command starts"
    );
}

#[test]
fn metric_name_is_bounded_and_value_must_be_finite() {
    let mut engine = engine("python script.py");
    let long_name = "x".repeat(65);
    let events = events_at(
        &mut engine,
        &format!("::sentinal metric name={long_name} value=1.0"),
        0,
    );
    assert!(events.is_empty());
    let events = events_at(&mut engine, "::sentinal metric name=loss value=inf", 10_000);
    assert!(events.is_empty());
}

#[test]
fn a_flood_of_metric_lines_is_rate_limited_per_name() {
    let mut engine = engine("python script.py");
    let mut events = Vec::new();
    for index in 0..100 {
        events.extend(events_at(
            &mut engine,
            &format!("::sentinal metric name=loss value={index}"),
            index,
        ));
        events.extend(events_at(
            &mut engine,
            &format!("::sentinal metric name=accuracy value={index}"),
            index,
        ));
    }
    let metrics = events
        .iter()
        .filter(|event| matches!(event, EventKind::MetricReported { .. }))
        .count();
    assert_eq!(metrics, 2, "one per name in 100 ms: {events:?}");
    // Five seconds later the same name reports again.
    let later = events_at(&mut engine, "::sentinal metric name=loss value=0.1", 5_100);
    assert_eq!(later.len(), 1);
}

#[test]
fn malformed_or_oversized_fields_reject_the_whole_line() {
    let mut engine = engine("python script.py");
    let long_unit = "u".repeat(33);
    for (index, line) in [
        "::sentinal progress current=3 total=x unit=task".to_string(),
        "::sentinal progress current=x total=5".to_string(),
        format!("::sentinal progress current=1 total=5 unit={long_unit}"),
        "::sentinal progress current=1 total=5 unit=a/b".to_string(),
        "::sentinal metric name=lo$s value=1".to_string(),
        "::sentinal metric name=loss value=abc".to_string(),
    ]
    .iter()
    .enumerate()
    {
        let at = i64::try_from(index).unwrap() * 10_000;
        assert!(
            events_at(&mut engine, line, at).is_empty(),
            "accepted: {line}"
        );
    }
}
