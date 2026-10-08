//! Deterministic Health behavior tests.

use protocol::{DurationMs, EventKind, HealthLevel, HealthOverall, Timestamp};
use watcher::{HealthTracker, Observation};

#[test]
fn healthy_rating_has_progress_evidence() {
    let mut tracker = HealthTracker::new();
    let health = tracker.observe(
        &Observation::output(Timestamp(1), "Epoch 7/10"),
        &[EventKind::ProgressUpdated {
            current: 7,
            total: Some(10),
            unit: "epoch".into(),
        }],
    );
    assert_eq!(health.overall, HealthOverall::Healthy);
    assert_eq!(health.progress, Some(70.0));
    assert_eq!(health.activity, HealthLevel::Good);
    assert!(!health.evidence.is_empty());
}

#[test]
fn warning_rating_lists_warning_evidence() {
    let mut tracker = HealthTracker::new();
    let health = tracker.observe(
        &Observation::output(Timestamp(1), "WARNING: retrying"),
        &[EventKind::WarningDetected {
            message: "WARNING: retrying".into(),
        }],
    );
    assert_eq!(health.overall, HealthOverall::Warning);
    assert!(health
        .evidence
        .iter()
        .any(|value| value.contains("WARNING")));
}

#[test]
fn stalled_rating_lists_quiet_resource_evidence() {
    let mut tracker = HealthTracker::new();
    let health = tracker.observe(
        &Observation::metrics(Timestamp(600_000), 1.0, Some(0.0)),
        &[EventKind::PossiblyStalled {
            silent_for_ms: DurationMs(600_000),
        }],
    );
    assert_eq!(health.overall, HealthOverall::PossiblyStalled);
    assert_eq!(health.stability, HealthLevel::Warning);
    assert!(health.evidence.iter().any(|value| value.contains("quiet")));
}

#[test]
fn failed_rating_counts_errors_and_lists_evidence() {
    let mut tracker = HealthTracker::new();
    let health = tracker.observe(
        &Observation::output(Timestamp(1), "error: build failed"),
        &[EventKind::ErrorDetected {
            message: "error: build failed".into(),
        }],
    );
    assert_eq!(health.overall, HealthOverall::Failed);
    assert_eq!(health.errors, 1);
    assert_eq!(health.stability, HealthLevel::Critical);
    assert!(health
        .evidence
        .iter()
        .any(|value| value.contains("build failed")));
}

#[test]
fn multi_core_cpu_above_100_percent_is_judged_by_machine_share() {
    // A tokenizer on 6 of 12 cores: 538% per-core, 45% of the Machine.
    let mut tracker = HealthTracker::new();
    let busy = watcher::MetricsObservation {
        cpu_percent: 538.1,
        cpu_machine_percent: Some(44.8),
        gpu_percent: None,
        ram_percent: Some(20.0),
        vram_percent: None,
    };
    let health = tracker.observe(
        &Observation {
            at: Timestamp(1),
            kind: watcher::ObservationKind::MetricsSample(busy),
        },
        &[],
    );
    assert_eq!(health.resources, HealthLevel::Good);
    assert!(
        !health.evidence.iter().any(|value| value.contains("538")),
        "{:?}",
        health.evidence
    );
}

#[test]
fn a_machine_actually_saturated_is_still_critical() {
    let mut tracker = HealthTracker::new();
    let saturated = watcher::MetricsObservation {
        cpu_percent: 1180.0,
        cpu_machine_percent: Some(98.3),
        gpu_percent: None,
        ram_percent: None,
        vram_percent: None,
    };
    let health = tracker.observe(
        &Observation {
            at: Timestamp(1),
            kind: watcher::ObservationKind::MetricsSample(saturated),
        },
        &[],
    );
    assert_eq!(health.resources, HealthLevel::Critical);
}
