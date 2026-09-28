//! Real process-table metric sampling integration coverage.

#![allow(clippy::unwrap_used)]

use daemon::metrics::MetricsSampler;
use protocol::{SessionId, Timestamp};

#[test]
#[ignore = "reads the host process table"]
fn sysinfo_process_sampling_integration() {
    let mut sampler = MetricsSampler::new();
    let snapshot = sampler.sample(
        Timestamp(1_790_000_000_000),
        &[(SessionId(1), std::process::id())],
    );
    assert!(snapshot.machine.ram_total_bytes > 0);
    assert!(snapshot
        .processes
        .iter()
        .any(|process| process.pid == std::process::id()));
    assert_eq!(snapshot.sessions.len(), 1);
}
