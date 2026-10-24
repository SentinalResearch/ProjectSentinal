//! Milestone 4.H health storage.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{
    observability::{HealthLabel, HealthMetric, HealthSample, IncidentKind},
    MachineId, Timestamp,
};
use store::{
    health::{Histogram, MAX_SAMPLES_PER_MINUTE, RAW_RETENTION_MS, SUMMARY_RETENTION_MS},
    Store,
};

const T0: i64 = 1_792_771_200_000; // a minute boundary

fn machine() -> MachineId {
    MachineId("01J9Z3K8V6Q2T7M4X5C0RNB8DA".into())
}

fn echo(at: i64, value: f64) -> HealthSample {
    HealthSample {
        machine_id: machine(),
        metric: HealthMetric::TypingEchoMs,
        label: None,
        at: Timestamp(at),
        value,
    }
}

#[test]
fn percentiles_are_within_the_histogram_resolution() {
    let mut histogram = Histogram::default();
    for value in 1..=1000 {
        histogram.add(f64::from(value));
    }
    let stat = histogram.stat().unwrap();
    assert_eq!((stat.count, stat.min, stat.max), (1000, 1.0, 1000.0));
    assert!((stat.p50 - 500.0).abs() / 500.0 < 0.1, "p50 {}", stat.p50);
    assert!((stat.p95 - 950.0).abs() / 950.0 < 0.1, "p95 {}", stat.p95);
    let mut one = Histogram::default();
    one.add(42.0);
    let stat = one.stat().unwrap();
    assert_eq!(
        (stat.p50, stat.p95),
        (42.0, 42.0),
        "clamped to the one value"
    );
}

#[test]
fn samples_merge_per_minute_and_across_a_range() {
    let store = Store::open_in_memory().unwrap();
    let samples: Vec<_> = (0..60)
        .map(|i| echo(T0 + i * 1_000, 10.0 + f64::from(i as u8)))
        .collect();
    store.record_health_samples(&samples).unwrap();
    // A late sample for the same minute, stored after the others.
    store
        .record_health_samples(&[echo(T0 + 30_000, 200.0)])
        .unwrap();
    store
        .record_health_samples(&[echo(T0 + 60_000, 12.0)])
        .unwrap();

    let rows = store
        .health_stats(Timestamp(T0), Timestamp(T0 + 120_000))
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].stat.count, 62);
    assert_eq!(rows[0].stat.max, 200.0);
    let only_first = store
        .health_stats(Timestamp(T0), Timestamp(T0 + 60_000))
        .unwrap();
    assert_eq!(only_first[0].stat.count, 61);

    let series = store
        .health_series(
            &machine(),
            HealthMetric::TypingEchoMs,
            None,
            Timestamp(T0),
            Timestamp(T0 + 120_000),
        )
        .unwrap();
    assert_eq!(series.len(), 2);
    assert_eq!(series[1].at, Timestamp(T0 + 60_000));
}

#[test]
fn the_per_minute_cap_counts_what_it_drops() {
    let store = Store::open_in_memory().unwrap();
    let samples: Vec<_> = (0..MAX_SAMPLES_PER_MINUTE + 5)
        .map(|i| echo(T0 + i * 100, 5.0))
        .collect();
    let insert = store.record_health_samples(&samples).unwrap();
    assert_eq!((insert.stored, insert.dropped), (120, 5));
    let rows = store
        .health_stats(Timestamp(T0), Timestamp(T0 + 60_000))
        .unwrap();
    assert_eq!((rows[0].stat.count, rows[0].dropped), (120, 5));
}

#[test]
fn labels_are_kept_apart() {
    let store = Store::open_in_memory().unwrap();
    let route = |label, value| HealthSample {
        machine_id: machine(),
        metric: HealthMetric::RouteMs,
        label: Some(label),
        at: Timestamp(T0),
        value,
    };
    store
        .record_health_samples(&[
            route(HealthLabel::RoutePing, 600.0),
            route(HealthLabel::RouteSnapshot, 3.0),
        ])
        .unwrap();
    let rows = store
        .health_stats(Timestamp(T0), Timestamp(T0 + 60_000))
        .unwrap();
    let labels: Vec<_> = rows.iter().map(|row| row.label).collect();
    assert_eq!(
        labels,
        vec![
            Some(HealthLabel::RoutePing),
            Some(HealthLabel::RouteSnapshot)
        ]
    );
}

#[test]
fn retention_drops_raw_after_a_day_and_summaries_after_thirty() {
    let store = Store::open_in_memory().unwrap();
    store.record_health_samples(&[echo(T0, 5.0)]).unwrap();
    store
        .record_health_moment(
            &machine(),
            IncidentKind::Restarted,
            Timestamp(T0),
            Some("0.3.0"),
        )
        .unwrap();

    store
        .prune_health(Timestamp(T0 + RAW_RETENTION_MS))
        .unwrap();
    assert_eq!(
        store
            .health_stats(Timestamp(0), Timestamp(i64::MAX))
            .unwrap()
            .len(),
        1,
        "summary kept"
    );
    let before = store.health_storage_bytes().unwrap();

    store
        .prune_health(Timestamp(T0 + RAW_RETENTION_MS + 1))
        .unwrap();
    assert!(
        store.health_storage_bytes().unwrap() < before,
        "raw sample gone"
    );
    assert_eq!(
        store
            .health_stats(Timestamp(0), Timestamp(i64::MAX))
            .unwrap()
            .len(),
        1
    );

    store
        .prune_health(Timestamp(T0 + SUMMARY_RETENTION_MS + 60_001))
        .unwrap();
    assert!(store
        .health_stats(Timestamp(0), Timestamp(i64::MAX))
        .unwrap()
        .is_empty());
    assert!(store.health_incidents(Timestamp(0)).unwrap().is_empty());
    assert_eq!(store.health_storage_bytes().unwrap(), 0);
}

#[test]
fn incidents_open_once_close_and_reject_free_text() {
    let store = Store::open_in_memory().unwrap();
    let m = machine();
    assert!(store
        .open_health_incident(&m, IncidentKind::Relayed, Timestamp(T0), Some("fra"))
        .unwrap());
    assert!(!store
        .open_health_incident(&m, IncidentKind::Relayed, Timestamp(T0 + 1), Some("nyc"))
        .unwrap());
    assert!(store
        .open_health_incident(
            &m,
            IncidentKind::Offline,
            Timestamp(T0 + 2),
            Some("/home/me/x")
        )
        .unwrap());
    assert!(store
        .close_health_incident(&m, IncidentKind::Relayed, Timestamp(T0 + 10))
        .unwrap());
    assert!(!store
        .close_health_incident(&m, IncidentKind::Relayed, Timestamp(T0 + 11))
        .unwrap());

    let incidents = store.health_incidents(Timestamp(T0)).unwrap();
    assert_eq!(incidents.len(), 2);
    assert_eq!(incidents[0].kind, IncidentKind::Offline);
    assert_eq!(incidents[0].ended_at, None);
    assert_eq!(incidents[0].detail, None, "a path is not a valid detail");
    assert_eq!(incidents[1].detail.as_deref(), Some("fra"));
    assert_eq!(incidents[1].ended_at, Some(Timestamp(T0 + 10)));
}

#[test]
fn an_existing_database_migrates_to_the_health_tables() {
    let path =
        std::env::temp_dir().join(format!("sentinal-health-migrate-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&path);
    drop(Store::open(&path).unwrap());
    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 11);
    store.record_health_samples(&[echo(T0, 5.0)]).unwrap();
    drop(store);
    let _ = std::fs::remove_file(&path);
}
