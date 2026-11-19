#![allow(clippy::unwrap_used)]
//! Time left from whatever a Command shows (B1).

use protocol::{
    Command, CommandId, EstimateBasis, EstimateConfidence, MachineId, SessionId, Timestamp,
};
use watcher::estimate::Estimator;
use watcher::{EventEngine, Observation, ObservationKind};

fn started() -> Estimator {
    let mut e = Estimator::default();
    e.start(Timestamp(0));
    e
}

#[test]
fn a_steady_run_counts_down_to_its_finish() {
    // 40 epochs, one every 1.5 s.
    let mut e = started();
    for epoch in 1..=10 {
        e.progress(Timestamp(epoch * 1_500), epoch as u64, Some(40));
    }
    let estimate = e.estimate(Timestamp(15_000)).unwrap();
    assert_eq!(estimate.basis, EstimateBasis::Counts);
    assert_eq!(estimate.confidence, EstimateConfidence::High);
    // 30 epochs × 1.5 s = 45 s.
    assert_eq!(estimate.left_secs, Some(45));
    assert_eq!(estimate.per_minute, Some(40.0));
    // Between updates the estimate keeps counting down.
    assert_eq!(e.estimate(Timestamp(25_000)).unwrap().left_secs, Some(35));
}

#[test]
fn one_burst_or_pause_does_not_swing_it() {
    let mut e = started();
    let times = [
        1_000, 2_000, 3_000, 3_050, 4_000, 5_000, 9_000, 10_000, 11_000,
    ];
    for (step, at) in times.iter().enumerate() {
        e.progress(Timestamp(*at), step as u64 + 1, Some(100));
    }
    let left = e.estimate(Timestamp(11_000)).unwrap().left_secs.unwrap();
    // About one step a second: 91 steps left is roughly 90 s, not 5 or 900.
    assert!((60..=120).contains(&left), "{left}");
}

#[test]
fn counts_that_stop_are_a_stall_not_a_countdown() {
    let mut e = started();
    for step in 1..=5 {
        e.progress(Timestamp(step * 2_000), step as u64, Some(50));
    }
    let estimate = e.estimate(Timestamp(10_000 + 120_000)).unwrap();
    assert_eq!(estimate.left_secs, None);
    assert_eq!(estimate.stalled_secs, Some(120));
}

#[test]
fn a_new_total_or_a_count_going_back_starts_over() {
    let mut e = started();
    for step in 1..=5 {
        e.progress(Timestamp(step * 1_000), step as u64, Some(10));
    }
    // Phase two: a different total.
    e.progress(Timestamp(6_000), 1, Some(200));
    assert!(
        e.estimate(Timestamp(6_000)).is_none(),
        "one point is no pace"
    );
    e.progress(Timestamp(7_000), 2, Some(200));
    let left = e.estimate(Timestamp(7_000)).unwrap().left_secs.unwrap();
    assert_eq!(left, 195, "198 left at 1 a second, rounded to 15 s");
}

#[test]
fn without_counts_the_command_s_history_says_how_long_it_usually_takes() {
    let mut e = started();
    e.set_history(&[240_000, 250_000, 260_000]);
    let estimate = e.estimate(Timestamp(60_000)).unwrap();
    assert_eq!(estimate.basis, EstimateBasis::History);
    assert_eq!(estimate.confidence, EstimateConfidence::Medium);
    assert_eq!(estimate.typical_secs, Some(255));
    assert_eq!(estimate.runs, Some(3));
    assert_eq!(estimate.left_secs, Some(195));
    assert!(!estimate.longer_than_usual);

    let late = e.estimate(Timestamp(500_000)).unwrap();
    assert_eq!(late.left_secs, None);
    assert!(late.longer_than_usual);
}

#[test]
fn with_nothing_to_go_on_there_is_no_estimate() {
    let e = started();
    assert!(e.estimate(Timestamp(5_000)).is_none());
    assert!(Estimator::default().estimate(Timestamp(5_000)).is_none());
}

#[test]
fn the_engine_estimates_from_any_progress_line_and_forgets_it_at_the_end() {
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("m".into()),
        "python train.py",
        "/work",
        Timestamp(0),
    );
    let mut engine = EventEngine::new(None, Vec::new());
    engine.observe(&Observation {
        at: Timestamp(0),
        kind: ObservationKind::CommandStarted(command.clone()),
    });
    let mut last = None;
    for epoch in 1..=6 {
        let update = engine.observe(&Observation::output(
            Timestamp(epoch * 2_000),
            format!("::sentinal progress current={epoch} total=20 unit=epoch"),
        ));
        last = update.estimate;
    }
    assert_eq!(
        last.unwrap().left_secs,
        Some(30),
        "14 epochs × 2 s, rounded to 5 s"
    );
    let finished = engine.observe(&Observation {
        at: Timestamp(13_000),
        kind: ObservationKind::CommandFinished(command),
    });
    assert!(finished.estimate.is_none());
}
