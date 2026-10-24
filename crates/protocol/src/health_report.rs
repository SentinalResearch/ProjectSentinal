//! Owner-only health views (milestone 4.H §2.1): the summary behind
//! Settings → Diagnostics, the incident list, and the downloadable report.

use crate::{
    link::MachineRole,
    observability::{HealthIncident, HealthLabel, HealthMetric, HealthStat, IncidentKind},
    MachineId, Os, Timestamp,
};
use serde::{Deserialize, Serialize};

/// How far back a view looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub enum HealthRange {
    /// Last hour.
    #[serde(rename = "1h")]
    Hour,
    /// Last 24 hours.
    #[serde(rename = "24h")]
    Day,
    /// Last 7 days.
    #[serde(rename = "7d")]
    Week,
    /// Last 30 days.
    #[serde(rename = "30d")]
    Month,
}

impl HealthRange {
    /// Length in milliseconds.
    pub fn millis(self) -> i64 {
        const HOUR: i64 = 3_600_000;
        match self {
            Self::Hour => HOUR,
            Self::Day => 24 * HOUR,
            Self::Week => 7 * 24 * HOUR,
            Self::Month => 30 * 24 * HOUR,
        }
    }
}

/// One metric's summary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MetricStat {
    /// Metric.
    pub metric: HealthMetric,
    /// Label, for labelled metrics.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<HealthLabel>,
    /// Distribution (or total, for counters).
    pub stat: HealthStat,
    /// Samples refused by the per-minute cap.
    pub dropped: u32,
}

/// One chart point.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SeriesPoint {
    /// Period start.
    pub at: Timestamp,
    /// Samples in the period.
    pub count: u32,
    /// Median.
    pub p50: f64,
    /// 95th percentile.
    pub p95: f64,
}

/// One chart: a metric over the range, at most 300 points.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MetricSeries {
    /// Metric.
    pub metric: HealthMetric,
    /// Points, oldest first.
    pub points: Vec<SeriesPoint>,
}

/// Metrics charted on the health page, in order.
pub const CHARTED_METRICS: [HealthMetric; 5] = [
    HealthMetric::TypingEchoMs,
    HealthMetric::TerminalOpenMs,
    HealthMetric::LinkRttMs,
    HealthMetric::DaemonRssBytes,
    HealthMetric::UploadDelayMs,
];

/// One Machine on the health page.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MachineHealth {
    /// Machine.
    pub machine_id: MachineId,
    /// Owner-set display name.
    pub display_name: String,
    /// Control or worker.
    pub role: MachineRole,
    /// Last reported daemon version.
    pub daemon_version: Option<String>,
    /// Whether the daemon reports its own health (false for older releases).
    pub reports_health: bool,
    /// The last 5 minutes.
    pub latest: Vec<MetricStat>,
    /// The whole range.
    pub stats: Vec<MetricStat>,
    /// Charts for [`CHARTED_METRICS`] that have data.
    pub series: Vec<MetricSeries>,
}

/// `GET /api/v1/health/summary`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct HealthSummary {
    /// Range shown.
    pub range: HealthRange,
    /// When built.
    pub generated_at: Timestamp,
    /// Approximate bytes of health data on the control.
    pub storage_bytes: u64,
    /// Control first, then workers.
    pub machines: Vec<MachineHealth>,
}

/// `GET /api/v1/health/incidents`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct HealthIncidents {
    /// Newest first, at most 200.
    pub incidents: Vec<HealthIncident>,
}

/// Whether a report names Machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ReportNames {
    /// "Machine 1", "Machine 2", … (the default).
    Anonymous,
    /// Owner-set display names.
    Real,
}

/// One Machine in a report; identified only by `name`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ReportMachine {
    /// "Machine 1" or the display name.
    pub name: String,
    /// Control or worker.
    pub role: MachineRole,
    /// Operating system, when known.
    pub os: Option<Os>,
    /// Daemon version.
    pub daemon_version: Option<String>,
    /// Summaries over the range.
    pub stats: Vec<MetricStat>,
}

/// One incident in a report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ReportIncident {
    /// [`ReportMachine::name`].
    pub machine: String,
    /// What happened.
    pub kind: IncidentKind,
    /// Start.
    pub started_at: Timestamp,
    /// End, or `None` while ongoing.
    pub ended_at: Option<Timestamp>,
    /// Relay region, version, or error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// How complete the data is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ReportCoverage {
    /// Samples summarised.
    pub samples: u64,
    /// Samples refused by the per-minute cap.
    pub dropped: u64,
}

/// `GET /api/v1/health/report`: what the owner downloads and shares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct HealthReport {
    /// Report format version.
    pub schema_version: u32,
    /// When built.
    pub generated_at: Timestamp,
    /// Range covered.
    pub range: HealthRange,
    /// Whether Machines are named.
    pub names: ReportNames,
    /// Control first, then workers.
    pub machines: Vec<ReportMachine>,
    /// Newest first, at most 200.
    pub incidents: Vec<ReportIncident>,
    /// Sample counts.
    pub coverage: ReportCoverage,
}

/// Current [`HealthReport::schema_version`].
pub const HEALTH_REPORT_VERSION: u32 = 1;
