//! A training script's own startup output must not become its training state.
//!
//! `logs/training/trl_sft_startup.log` reconstructs the start of a real TRL
//! fine-tune (2026-10-04): it prints a sample training log from its dataset,
//! then Hugging Face loading/preprocessing bars, then the real training bar.

#![allow(clippy::unwrap_used)]

use protocol::{Command, CommandId, EventKind, LossTrend, MachineId, SessionId, Timestamp};
use watcher::{training::TrainingAdapter, Observation, WatcherAdapter};

fn replay() -> (TrainingAdapter, Vec<EventKind>) {
    let mut adapter = TrainingAdapter::new();
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("m".into()),
        "python eval/finetune/train_sft.py --model Qwen/Qwen3-0.6B",
        "/w",
        Timestamp(0),
    );
    assert!(adapter.matches(&command));
    let text = std::fs::read_to_string("tests/logs/training/trl_sft_startup.log").unwrap();
    let mut events = Vec::new();
    for (index, line) in text.split_inclusive(['\n', '\r']).enumerate() {
        let observation =
            Observation::output(Timestamp(index as i64), line.trim_end_matches(['\n', '\r']));
        events.extend(adapter.observe(&observation));
    }
    (adapter, events)
}

#[test]
fn loading_and_preprocessing_bars_are_not_training_progress() {
    let (_, events) = replay();
    for event in &events {
        if let EventKind::ProgressUpdated { total, .. } = event {
            assert!(
                !matches!(total, Some(311) | Some(4040) | Some(286)),
                "a data/model loading bar became progress: {event:?}"
            );
        }
    }
}

#[test]
fn the_real_run_replaces_numbers_read_from_a_printed_sample() {
    let (adapter, events) = replay();
    let last_progress = events.iter().rev().find_map(|event| match event {
        EventKind::ProgressUpdated {
            current,
            total,
            unit,
        } => Some((*current, *total, unit.clone())),
        _ => None,
    });
    assert_eq!(last_progress, Some((20, Some(759), "step".into())));
    let summary = adapter.summary().unwrap();
    assert_eq!(summary.loss, Some(0.2134));
    assert_eq!(
        summary.first_loss,
        Some(0.6001),
        "\"loss from\" must come from the real run, not the sample"
    );
    assert_eq!(summary.learning_rate, Some(1.5e-05));
    assert_eq!(summary.trend, LossTrend::Unknown);
}

#[test]
fn a_bare_fraction_outside_a_progress_bar_is_not_progress() {
    let mut adapter = TrainingAdapter::new();
    for line in [
        "Loading checkpoint shards 4/4 done",
        "seed 10/04 run",
        "split ratio 8/2",
    ] {
        let events = adapter.observe(&Observation::output(Timestamp(0), line));
        assert!(events.is_empty(), "{line}: {events:?}");
    }
}

#[test]
fn coloured_keras_and_iter_progress_are_still_read() {
    let mut adapter = TrainingAdapter::new();
    let cases = [
        (
            "\x1b[2m 80%|████████  | 4/5 [00:42<00:21, 11.8it/s, loss=0.719]\x1b[0m",
            (4, 5, "step"),
        ),
        (
            "120/500 [==============>...............] - ETA: 12s - loss: 0.3301",
            (120, 500, "step"),
        ),
        ("iter 120/1000 | reward 12.5 | kl 0.03", (120, 1000, "iter")),
    ];
    for (line, (current, total, unit)) in cases {
        let events = adapter.observe(&Observation::output(Timestamp(0), line));
        let expected = EventKind::ProgressUpdated {
            current,
            total: Some(total),
            unit: unit.into(),
        };
        assert!(events.contains(&expected), "{line:?}: {events:?}");
    }
}
