//! Machine-wide metric history across raw samples and minute averages.

#![allow(clippy::unwrap_used)]

use protocol::{SessionId, Timestamp};
use store::{RawMetricSample, Store};

fn sample(session: Option<u64>, at: i64, cpu: f32, gpu: Option<f32>) -> RawMetricSample {
    RawMetricSample {
        session_id: session.map(SessionId),
        at: Timestamp(at),
        cpu_percent: cpu,
        ram_used_bytes: 400,
        ram_total_bytes: 1_000,
        gpu_percent: gpu,
        vram_used_bytes: gpu.map(|_| 10),
        vram_total_bytes: gpu.map(|_| 100),
    }
}

const HOUR: i64 = 3_600_000;

#[test]
fn history_buckets_machine_samples_from_minute_and_raw_tables_oldest_first() {
    let store = Store::open_in_memory().unwrap();
    // Two old Machine samples become one minute row after downsampling.
    store
        .insert_raw_metric(&sample(None, 60_000, 20.0, None))
        .unwrap();
    store
        .insert_raw_metric(&sample(None, 70_000, 40.0, None))
        .unwrap();
    // A Session sample in the same minute must not leak into Machine history.
    store
        .insert_raw_metric(&sample(Some(1), 65_000, 99.0, None))
        .unwrap();
    store.downsample_metrics(Timestamp(2 * HOUR)).unwrap();
    // Recent raw samples in one 15 s bucket.
    store
        .insert_raw_metric(&sample(None, 2 * HOUR + 1_000, 10.0, Some(80.0)))
        .unwrap();
    store
        .insert_raw_metric(&sample(None, 2 * HOUR + 2_000, 30.0, Some(90.0)))
        .unwrap();

    let points = store.machine_metric_points(Timestamp(0), 15_000).unwrap();

    assert_eq!(points.len(), 2);
    assert_eq!(points[0].at, Timestamp(60_000));
    assert_eq!(points[0].cpu_percent, 30.0);
    assert_eq!(points[0].gpu_percent, None);
    assert_eq!(points[1].at, Timestamp(2 * HOUR));
    assert_eq!(points[1].cpu_percent, 20.0);
    assert_eq!(points[1].gpu_percent, Some(85.0));
    assert_eq!(points[1].ram_total_bytes, 1_000);

    let recent = store
        .machine_metric_points(Timestamp(HOUR), 15_000)
        .unwrap();
    assert_eq!(recent.len(), 1);
}
