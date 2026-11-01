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

fn command(id: u64, text: &str, at: i64) -> protocol::Command {
    protocol::Command::start(
        protocol::CommandId(id),
        protocol::SessionId(1),
        protocol::MachineId("machine".into()),
        text,
        "/work",
        Timestamp(at),
    )
}

fn observe_kind(
    tracker: &mut HealthTracker,
    at: i64,
    kind: watcher::ObservationKind,
) -> protocol::Health {
    tracker.observe(
        &Observation {
            at: Timestamp(at),
            kind,
        },
        &[],
    )
}

#[test]
fn a_new_command_clears_the_previous_commands_failure() {
    // `vllm serve` failed once (not on PATH), then ran fine after `conda
    // activate`: the Session read RUNNING and FAILED at the same time forever.
    let mut tracker = HealthTracker::new();
    let mut failed = command(1, "vllm serve Qwen/Qwen3-4B", 1);
    failed.finish(127, Timestamp(2)).unwrap();
    let health = observe_kind(
        &mut tracker,
        2,
        watcher::ObservationKind::CommandFinished(failed),
    );
    assert_eq!(health.overall, HealthOverall::Failed);

    let health = observe_kind(
        &mut tracker,
        3,
        watcher::ObservationKind::CommandStarted(command(2, "vllm serve Qwen/Qwen3-4B", 3)),
    );
    assert_eq!(health.errors, 0);
    assert_eq!(health.overall, HealthOverall::Healthy);
    assert!(
        !health.evidence.iter().any(|value| value.contains("127")),
        "{:?}",
        health.evidence
    );
}

#[test]
fn a_failure_in_the_running_command_still_fails_it() {
    let mut tracker = HealthTracker::new();
    observe_kind(
        &mut tracker,
        1,
        watcher::ObservationKind::CommandStarted(command(1, "make", 1)),
    );
    let health = tracker.observe(
        &Observation::output(Timestamp(2), "error: build failed"),
        &[EventKind::ErrorDetected {
            message: "error: build failed".into(),
        }],
    );
    assert_eq!(health.overall, HealthOverall::Failed);
}

#[test]
fn repeated_samples_keep_one_evidence_line_per_kind() {
    // The summary used to alternate "resource usage reached 92.1%" with
    // "terminal output observed" until all 16 slots were near-duplicates.
    let mut tracker = HealthTracker::new();
    let mut health = None;
    for step in 0..20_i64 {
        let busy = watcher::MetricsObservation {
            cpu_percent: 0.0,
            cpu_machine_percent: None,
            gpu_percent: Some(91.0 + step as f32 / 10.0),
            ram_percent: None,
            vram_percent: None,
        };
        observe_kind(
            &mut tracker,
            step * 2,
            watcher::ObservationKind::MetricsSample(busy),
        );
        health = Some(tracker.observe(
            &Observation::output(Timestamp(step * 2 + 1), "INFO request"),
            &[],
        ));
    }
    let evidence = health.unwrap().evidence;
    let resource_lines: Vec<_> = evidence
        .iter()
        .filter(|value| value.contains("usage reached"))
        .collect();
    assert_eq!(
        resource_lines,
        ["resource usage reached 92.9%"],
        "{evidence:?}"
    );
    assert_eq!(
        evidence
            .iter()
            .filter(|value| *value == "terminal output observed")
            .count(),
        1,
        "{evidence:?}"
    );
}

#[test]
fn resource_events_name_the_resource_plainly() {
    let mut tracker = HealthTracker::new();
    let health = tracker.observe(
        &Observation::output(Timestamp(1), "busy"),
        &[EventKind::ResourceHigh {
            resource: protocol::Resource::Cpu,
            percent: 97.0,
        }],
    );
    assert!(
        health
            .evidence
            .iter()
            .any(|value| value == "CPU usage reached 97.0%"),
        "{:?}",
        health.evidence
    );
}
