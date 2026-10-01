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
