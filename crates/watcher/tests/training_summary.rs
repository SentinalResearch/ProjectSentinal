//! The training adapter's structured state becomes a wire summary.

#![allow(clippy::unwrap_used)]

use protocol::{Command, CommandId, LossTrend, MachineId, SessionId, Timestamp};
use watcher::{training::TrainingAdapter, Observation, WatcherAdapter};

#[test]
fn summary_tracks_first_and_latest_loss_best_validation_and_trend() {
    let mut adapter = TrainingAdapter::new();
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("m".into()),
        "python train.py --config moe.yaml",
        "/w",
        Timestamp(0),
    );
    assert!(adapter.matches(&command));
    assert_eq!(adapter.summary(), None);
    for line in [
        "epoch 1/100 loss 1.840 lr 2.0e-4",
        "epoch 2/100 loss 0.900 val_loss 0.80",
        "epoch 3/100 loss 0.362 val_loss 0.41 lr 1.0e-4",
    ] {
        adapter.observe(&Observation::output(Timestamp(1), line));
    }

    let summary = adapter.summary().unwrap();
    assert_eq!(summary.first_loss, Some(1.84));
    assert_eq!(summary.loss, Some(0.362));
    assert_eq!(summary.best_val_loss, Some(0.41));
    assert_eq!(summary.learning_rate, Some(1.0e-4));
    assert_eq!(summary.trend, LossTrend::Improving);
}
