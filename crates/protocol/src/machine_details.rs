//! Machine page data: host and daemon facts, per-Session processes, and
//! Machine-wide metric history.

use crate::{link::ConnectionReport, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// Host, daemon, and link facts for one Machine's detail page.
///
/// Holds no secret: the link report names the link only by its opaque ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MachineDetails {
    /// Hostname as the operating system reports it.
    pub hostname: String,
    /// Operating system name and version, e.g. `"macOS 26.6.2"`.
    pub os_version: Option<String>,
    /// Kernel version, if known.
    pub kernel_version: Option<String>,
    /// Version of the running `sentinald`.
    pub daemon_version: String,
    /// When this daemon process started.
    pub started_at: Timestamp,
    /// Address the daemon API is bound to.
    pub bind: String,
    /// This Machine's link to a control, if the daemon has a link manager.
    pub link: Option<ConnectionReport>,
    /// Latest sampled process tree of each live Session.
    pub processes: Vec<SessionProcesses>,
}

/// The busiest processes under one Session's root process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionProcesses {
    /// The Session whose process tree this is.
    pub session_id: SessionId,
    /// Processes ordered by CPU use, highest first.
    pub processes: Vec<ProcessSummary>,
}

/// One process without its command line, which may contain secrets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProcessSummary {
    /// Operating system process ID.
    pub pid: u32,
    /// Parent process ID, if known.
    pub parent_pid: Option<u32>,
    /// Executable name.
    pub name: String,
    /// CPU use as a percentage of one core.
    pub cpu_percent: f32,
    /// Resident memory in bytes.
    pub rss_bytes: u64,
    /// GPU memory in bytes, if known.
    pub gpu_mem_bytes: Option<u64>,
}

/// How far back a Machine metrics history request reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MetricRange {
    /// The last hour.
    Hour,
    /// The last 24 hours.
    Day,
    /// The last 7 days.
    Week,
}

impl MetricRange {
    /// Length of the range in milliseconds.
    pub const fn span_ms(self) -> i64 {
        match self {
            MetricRange::Hour => 60 * 60 * 1_000,
            MetricRange::Day => 24 * 60 * 60 * 1_000,
            MetricRange::Week => 7 * 24 * 60 * 60 * 1_000,
        }
    }

    /// Bucket width so each range returns at most a few hundred points.
    pub const fn bucket_ms(self) -> i64 {
        match self {
            MetricRange::Hour => 15 * 1_000,
            MetricRange::Day => 5 * 60 * 1_000,
            MetricRange::Week => 30 * 60 * 1_000,
        }
    }
}

/// Machine-wide averages over one bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MachineMetricPoint {
    /// Start of the bucket.
    pub at: Timestamp,
    /// Average Machine CPU use as a percentage.
    pub cpu_percent: f32,
    /// Average RAM in use, in bytes.
    pub ram_used_bytes: u64,
    /// Total RAM, in bytes.
    pub ram_total_bytes: u64,
    /// Average highest-GPU utilisation, if a GPU was sampled.
    pub gpu_percent: Option<f32>,
    /// Average VRAM in use, in bytes, if a GPU was sampled.
    pub vram_used_bytes: Option<u64>,
    /// Total VRAM, in bytes, if a GPU was sampled.
    pub vram_total_bytes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_range_returns_a_bounded_number_of_points() {
        for range in [MetricRange::Hour, MetricRange::Day, MetricRange::Week] {
            let points = range.span_ms() / range.bucket_ms();
            assert!((200..=400).contains(&points), "{range:?}: {points}");
        }
    }
}
