#![allow(clippy::unwrap_used)]
//! Progress from any program no built-in adapter recognises (an adopted
//! `python gsm8k_pilot.py`, a shell script), using only explicit counters.

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

fn progress(command_text: &str, lines: &[&str]) -> Vec<(u64, Option<u64>, String)> {
    let mut engine = EventEngine::new(Some(&command(command_text)), Vec::new())
        .with_capture(CaptureLevel::ProcessLog);
    lines
        .iter()
        .enumerate()
        .flat_map(|(index, line)| {
            // Far enough apart that the 5 s progress rate limit never applies.
            let at = Timestamp(i64::try_from(index).unwrap() * 10_000);
            engine.observe(&Observation::output(at, *line)).events
        })
        .filter_map(|event| match event {
            EventKind::ProgressUpdated {
                current,
                total,
                unit,
            } => Some((current, total, unit)),
            _ => None,
        })
        .collect()
}

#[test]
fn a_labelled_counter_from_an_unrecognised_script_is_progress() {
    let events = progress(
        "python gsm8k_pilot.py",
        &[
            "task 3/50",
            "Processing sample 12 of 200",
            "[4/10] building",
        ],
    );
    assert_eq!(
        events,
        vec![
            (3, Some(50), "task".into()),
            (12, Some(200), "sample".into()),
            (4, Some(10), "item".into()),
        ]
    );
}

#[test]
fn a_tqdm_bar_with_any_label_is_progress_without_an_adapter() {
    let events = progress(
        "python gsm8k_pilot.py",
        &["Evaluating:  42%|████▏     | 21/50 [00:10<00:14,  2.05it/s]"],
    );
    assert_eq!(events, vec![(21, Some(50), "item".into())]);
}

#[test]
fn dates_ratios_and_impossible_counts_are_not_progress() {
    let events = progress(
        "python gsm8k_pilot.py",
        &[
            "run started 2026/11/03",
            "accuracy 41/50 on condition A",
            "task 7/0",
            "task 9/5",
            "see docs/1/2 for details",
        ],
    );
    assert_eq!(events, vec![]);
}

#[test]
fn a_selected_adapter_still_owns_progress() {
    // pytest prints "[ 40%]"-style output; the fallback must not add its own.
    let events = progress("pytest tests/", &["[3/10] collecting"]);
    assert_eq!(events, vec![]);
}

#[test]
fn a_session_without_a_command_reports_counters_too() {
    let mut engine = EventEngine::new(None, Vec::new()).with_capture(CaptureLevel::ProcessLog);
    let update = engine.observe(&Observation::output(Timestamp(0), "step 5/20"));
    assert!(update.events.contains(&EventKind::ProgressUpdated {
        current: 5,
        total: Some(20),
        unit: "step".into(),
    }));
}
