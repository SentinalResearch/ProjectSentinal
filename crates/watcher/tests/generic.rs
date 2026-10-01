//! Generic watcher behavior tests.

use protocol::{DurationMs, EventKind, Timestamp};
use watcher::{GenericAdapter, Observation, WatcherAdapter};

#[test]
fn prompt_requires_exact_shape_and_two_quiet_seconds() {
    let mut generic = GenericAdapter::new(DurationMs(600_000));
    assert!(generic
        .observe(&Observation::output(Timestamp(0), "Continue? [y/N]"))
        .is_empty());
    assert!(generic
        .observe(&Observation::tick(Timestamp(1_999)))
        .is_empty());
    assert!(matches!(
        generic.observe(&Observation::tick(Timestamp(2_000))).as_slice(),
        [EventKind::WaitingForInput { prompt: Some(prompt) }] if prompt == "Continue? [y/N]"
    ));

    let mut negative = GenericAdapter::new(DurationMs(600_000));
    negative.observe(&Observation::output(
        Timestamp(0),
        "the password is stored in a keychain",
    ));
    assert!(negative
        .observe(&Observation::tick(Timestamp(3_000)))
        .is_empty());
}

#[test]
fn idle_cpu_and_gpu_stall_but_busy_gpu_does_not() {
    let mut stalled = GenericAdapter::new(DurationMs(600_000));
    stalled.observe(&Observation::output(Timestamp(0), "working"));
    let events = stalled.observe(&Observation::metrics(Timestamp(600_000), 2.0, Some(1.0)));
    assert!(matches!(
        events.as_slice(),
        [EventKind::PossiblyStalled { .. }]
    ));

    let mut computing = GenericAdapter::new(DurationMs(600_000));
    computing.observe(&Observation::output(Timestamp(0), "working"));
    assert!(computing
        .observe(&Observation::metrics(Timestamp(600_000), 1.0, Some(96.0)))
        .is_empty());
}

#[test]
fn metrics_start_the_quiet_clock_when_no_output_has_ever_arrived() {
    let mut generic = GenericAdapter::new(DurationMs(600_000));
    assert!(generic
        .observe(&Observation::metrics(Timestamp(0), 1.0, None))
        .is_empty());
    assert!(matches!(
        generic
            .observe(&Observation::metrics(Timestamp(600_000), 1.0, None))
            .as_slice(),
        [EventKind::PossiblyStalled { .. }]
    ));
}

#[test]
fn generic_detects_errors_and_warnings() {
    let mut generic = GenericAdapter::new(DurationMs(600_000));
    let error = generic.observe(&Observation::output(
        Timestamp(0),
        "RuntimeError: CUDA out of memory",
    ));
    assert!(matches!(
        error.as_slice(),
        [EventKind::ErrorDetected { .. }]
    ));
    let warning = generic.observe(&Observation::output(Timestamp(1), "WARNING: retrying"));
    assert!(matches!(
        warning.as_slice(),
        [EventKind::WarningDetected { .. }]
    ));
}
