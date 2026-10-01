#![allow(clippy::unwrap_used)]
//! Capture-aware evidence: reduced capture never claims what it cannot see.

use protocol::{
    CaptureLevel, Command, CommandId, EventKind, HealthOverall, MachineId, SessionId, Timestamp,
};
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

fn replay(path: &str, command_text: &str, capture: CaptureLevel) -> Vec<EventKind> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut engine =
        EventEngine::new(Some(&command(command_text)), Vec::new()).with_capture(capture);
    text.split_inclusive(['\n', '\r'])
        .enumerate()
        .flat_map(|(index, line)| {
            engine
                .observe(&Observation::output(
                    Timestamp(i64::try_from(index).unwrap() * 1_000),
                    line.trim_end_matches(['\n', '\r']),
                ))
                .events
        })
        .collect()
}

#[test]
fn adopted_log_fixtures_produce_the_same_events_as_full_sessions() {
    for (path, text) in [
        ("tests/logs/training/lightning.log", "python train.py"),
        (
            "tests/logs/training/hf_trainer.log",
            "accelerate launch train.py",
        ),
        ("tests/logs/training/tqdm.log", "torchrun train_model.py"),
        ("tests/logs/pytest/pass.log", "pytest tests/"),
        ("tests/logs/pytest/fail.log", "pytest tests/"),
        ("tests/logs/server/vllm.log", "vllm serve model"),
        ("tests/logs/server/llama.log", "llama-server -m model.gguf"),
    ] {
        let full = replay(path, text, CaptureLevel::Full);
        let adopted = replay(path, text, CaptureLevel::ProcessLog);
        assert!(!full.is_empty(), "{path} produced no Events");
        assert_eq!(adopted, full, "{path}");
    }
}

#[test]
fn process_log_error_lines_are_events_but_health_is_not_failed() {
    let mut engine = EventEngine::new(None, Vec::new()).with_capture(CaptureLevel::ProcessLog);

    let update = engine.observe(&Observation::output(
        Timestamp(1),
        "Traceback (most recent call last):",
    ));

    assert!(update
        .events
        .iter()
        .any(|event| matches!(event, EventKind::ErrorDetected { .. })));
    assert_eq!(update.health.errors, 1);
    assert_eq!(update.health.overall, HealthOverall::Warning);
}

#[test]
fn full_error_lines_still_mark_health_failed() {
    let mut engine = EventEngine::new(None, Vec::new());
    let update = engine.observe(&Observation::output(
        Timestamp(1),
        "Traceback (most recent call last):",
    ));
    assert_eq!(update.health.overall, HealthOverall::Failed);
}

#[test]
fn falling_back_to_process_suppresses_output_derived_stall_claims() {
    let mut engine = EventEngine::new(None, Vec::new()).with_capture(CaptureLevel::ProcessLog);
    engine.observe(&Observation::output(Timestamp(0), "working"));

    engine.set_capture(CaptureLevel::Process);
    let update = engine.observe(&Observation::metrics(Timestamp(600_000), 1.0, Some(1.0)));

    assert!(update.events.is_empty(), "{:?}", update.events);
    assert_ne!(update.health.overall, HealthOverall::PossiblyStalled);
}

#[test]
fn process_log_quiet_log_with_idle_resources_may_report_a_stall() {
    let mut engine = EventEngine::new(None, Vec::new()).with_capture(CaptureLevel::ProcessLog);
    engine.observe(&Observation::output(Timestamp(0), "working"));
    let update = engine.observe(&Observation::metrics(Timestamp(600_000), 1.0, Some(1.0)));
    assert!(matches!(
        update.events.as_slice(),
        [EventKind::PossiblyStalled { .. }]
    ));
}
