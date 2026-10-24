//! How Sentinal itself is doing on the owner's Machines (milestone 4.H):
//! typing delay, terminal open time, link round trips, daemon resources,
//! incidents. Numbers, closed codes, versions, and Machine IDs only. Nothing
//! here can carry terminal output, keystrokes, Commands, paths, hostnames,
//! IPs, credentials, or free-form messages: every label is a closed enum and
//! the one free-ish field (`HealthIncident::detail`) is validated.

use crate::{MachineId, Timestamp};
use serde::{Deserialize, Serialize};

/// What a sample measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HealthMetric {
    /// Keystroke sent → next output from that terminal (browser).
    TypingEchoMs,
    /// Attach request → first screen drawn (browser).
    TerminalOpenMs,
    /// One page-load step, labelled with a [`HealthLabel`] load step (browser).
    PageLoadStepMs,
    /// Control → worker identity round trip (control).
    LinkRttMs,
    /// Worker's oldest unsent projection change when an upload succeeded (worker).
    UploadDelayMs,
    /// Failed projection uploads since the last report (worker).
    UploadFailures,
    /// `sentinald` resident memory.
    DaemonRssBytes,
    /// `sentinald` CPU use, percent of one core.
    DaemonCpuPercent,
    /// Live Sessions on the Machine.
    LiveSessions,
    /// Saved Session logs on disk.
    LogDiskBytes,
    /// Control request time, labelled with a [`HealthLabel`] route.
    RouteMs,
    /// WebSocket reconnects, labelled with a [`HealthLabel`] socket kind (browser).
    WsReconnects,
    /// Viewer redraws after falling behind (browser).
    ViewerResyncs,
    /// Errors, labelled with a [`HealthLabel`] error code.
    Errors,
}

impl HealthMetric {
    /// Every metric, in a stable order.
    pub const ALL: [HealthMetric; 14] = [
        Self::TypingEchoMs,
        Self::TerminalOpenMs,
        Self::PageLoadStepMs,
        Self::LinkRttMs,
        Self::UploadDelayMs,
        Self::UploadFailures,
        Self::DaemonRssBytes,
        Self::DaemonCpuPercent,
        Self::LiveSessions,
        Self::LogDiskBytes,
        Self::RouteMs,
        Self::WsReconnects,
        Self::ViewerResyncs,
        Self::Errors,
    ];

    /// Counters are summed over a period; the others are distributions.
    pub fn is_counter(self) -> bool {
        matches!(
            self,
            Self::UploadFailures | Self::WsReconnects | Self::ViewerResyncs | Self::Errors
        )
    }

    /// Metrics a browser may report through `POST /api/v1/health/samples`.
    pub fn browser_reported(self) -> bool {
        matches!(
            self,
            Self::TypingEchoMs
                | Self::TerminalOpenMs
                | Self::PageLoadStepMs
                | Self::WsReconnects
                | Self::ViewerResyncs
                | Self::Errors
        )
    }

    /// The largest plausible value; anything above is rejected, not clamped.
    pub fn max_value(self) -> f64 {
        match self {
            Self::TypingEchoMs => 10_000.0,
            Self::TerminalOpenMs | Self::PageLoadStepMs | Self::RouteMs | Self::LinkRttMs => {
                600_000.0
            }
            Self::UploadDelayMs => 86_400_000.0,
            Self::DaemonRssBytes | Self::LogDiskBytes => 1e15,
            Self::DaemonCpuPercent => 100_000.0,
            Self::LiveSessions => 100_000.0,
            Self::UploadFailures | Self::WsReconnects | Self::ViewerResyncs | Self::Errors => {
                1_000_000.0
            }
        }
    }

    /// Whether `label` is allowed (and required) for this metric.
    pub fn accepts(self, label: Option<HealthLabel>) -> bool {
        match (self, label) {
            (Self::PageLoadStepMs, Some(label)) => label.kind() == LabelKind::LoadStep,
            (Self::RouteMs, Some(label)) => label.kind() == LabelKind::Route,
            (Self::WsReconnects, Some(label)) => label.kind() == LabelKind::Socket,
            (Self::Errors, Some(label)) => label.kind() == LabelKind::ErrorCode,
            (Self::PageLoadStepMs | Self::RouteMs | Self::WsReconnects | Self::Errors, None) => {
                false
            }
            (_, label) => label.is_none(),
        }
    }
}

/// The closed set of labels a sample may carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HealthLabel {
    // Page-load steps (`web/src/data/load-timing.ts`).
    /// The app's script started.
    LoadScript,
    /// The control snapshot arrived.
    LoadSnapshot,
    /// Live updates connected.
    LoadLive,
    /// Sessions were shown.
    LoadShown,
    // Control routes.
    /// `GET /api/v1/control/snapshot`.
    RouteSnapshot,
    /// `GET /api/v1/machines`.
    RouteRegistry,
    /// `GET /api/v1/machines/{id}/ping`.
    RoutePing,
    /// Identity proof + upstream connect for a routed terminal.
    RouteTerminalSetup,
    // Browser socket kinds.
    /// The dashboard's live projection socket.
    SocketDashboard,
    /// A terminal's socket.
    SocketTerminal,
    /// A Machine request socket.
    SocketMachine,
    // Error codes.
    /// A terminal could not attach.
    ErrorAttach,
    /// Terminal input was refused or lost.
    ErrorInput,
    /// A Session could not be created.
    ErrorCreate,
    /// A Machine did not answer in time.
    ErrorTimeout,
    /// A Machine could not be reached.
    ErrorUnreachable,
    /// The browser session expired.
    ErrorAuthExpired,
    /// A projection upload failed.
    ErrorUpload,
    /// A heartbeat failed.
    ErrorHeartbeat,
    /// Writing a Session log failed.
    ErrorLogWrite,
    /// A database operation failed.
    ErrorStore,
    /// A PTY could not be started.
    ErrorPty,
    /// Anything else.
    ErrorOther,
}

/// Which family a [`HealthLabel`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelKind {
    /// A page-load step.
    LoadStep,
    /// A control route.
    Route,
    /// A browser socket kind.
    Socket,
    /// An error code.
    ErrorCode,
}

impl HealthLabel {
    /// The label's family.
    pub fn kind(self) -> LabelKind {
        use HealthLabel::*;
        match self {
            LoadScript | LoadSnapshot | LoadLive | LoadShown => LabelKind::LoadStep,
            RouteSnapshot | RouteRegistry | RoutePing | RouteTerminalSetup => LabelKind::Route,
            SocketDashboard | SocketTerminal | SocketMachine => LabelKind::Socket,
            _ => LabelKind::ErrorCode,
        }
    }
}

/// One measurement. Unknown fields are refused, so nothing extra can ride along.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(deny_unknown_fields)]
pub struct HealthSample {
    /// The Machine measured (for browser samples, the Machine the terminal or request was on).
    pub machine_id: MachineId,
    /// What was measured.
    pub metric: HealthMetric,
    /// Required for labelled metrics, absent otherwise ([`HealthMetric::accepts`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<HealthLabel>,
    /// When.
    pub at: Timestamp,
    /// Milliseconds, bytes, percent, or a count, per the metric.
    pub value: f64,
}

/// Why a [`HealthSample`] was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HealthSampleError {
    /// The metric needs a label of another family, or none.
    #[error("{metric:?} does not take label {label:?}")]
    Label {
        /// Metric.
        metric: HealthMetric,
        /// Offered label.
        label: Option<HealthLabel>,
    },
    /// Negative, not finite, or above [`HealthMetric::max_value`].
    #[error("{0:?} value out of range")]
    Value(HealthMetric),
    /// More than a day away from the receiver's clock.
    #[error("sample time is more than a day from now")]
    Time,
}

/// Samples more than this far from the receiver's clock are refused.
pub const MAX_SAMPLE_SKEW_MS: i64 = 86_400_000;

impl HealthSample {
    /// Check the label, value range, and time against `now`.
    pub fn validate(&self, now: Timestamp) -> Result<(), HealthSampleError> {
        if !self.metric.accepts(self.label) {
            return Err(HealthSampleError::Label {
                metric: self.metric,
                label: self.label,
            });
        }
        if !self.value.is_finite() || self.value < 0.0 || self.value > self.metric.max_value() {
            return Err(HealthSampleError::Value(self.metric));
        }
        if (self.at.0 - now.0).abs() > MAX_SAMPLE_SKEW_MS {
            return Err(HealthSampleError::Time);
        }
        Ok(())
    }
}

/// Most samples one browser upload may carry.
pub const MAX_BROWSER_SAMPLES: usize = 500;

/// Browser → control: `POST /api/v1/health/samples`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(deny_unknown_fields)]
pub struct HealthSampleBatch {
    /// At most [`MAX_BROWSER_SAMPLES`].
    pub samples: Vec<HealthSample>,
}

/// A daemon's view of itself, sent by workers in their heartbeat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct DaemonHealthReport {
    /// When sampled.
    pub at: Timestamp,
    /// `sentinald` resident memory.
    pub rss_bytes: u64,
    /// `sentinald` CPU, percent of one core.
    pub cpu_percent: f64,
    /// Sessions not yet finished.
    pub live_sessions: u32,
    /// Saved Session logs on disk.
    pub log_disk_bytes: u64,
    /// Since this daemon started.
    pub uptime_ms: u64,
    /// Daemon starts recorded on this install.
    pub restarts: u32,
    /// Oldest unsent change at the last successful upload, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_delay_ms: Option<u32>,
    /// Failed uploads since the last report.
    pub upload_failures: u32,
    /// Errors by code since the last report.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<HealthErrorCount>,
}

/// How many times one error code happened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct HealthErrorCount {
    /// Error code label.
    pub code: HealthLabel,
    /// Occurrences.
    pub count: u32,
}

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum IncidentKind {
    /// Heartbeats stopped.
    Offline,
    /// Online, but the dashboard copy was behind.
    Delayed,
    /// Tailscale reached the worker through a relay; `detail` is the region.
    Relayed,
    /// The daemon restarted; `detail` is the version it came back with.
    Restarted,
    /// A worker update failed; `detail` is the target version.
    UpdateFailed,
    /// Many errors in a short time; `detail` is the error code.
    ErrorBurst,
}

/// A period something was wrong (or one moment, for restarts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct HealthIncident {
    /// Machine.
    pub machine_id: MachineId,
    /// What happened.
    pub kind: IncidentKind,
    /// Start.
    pub started_at: Timestamp,
    /// End, or `None` while ongoing.
    pub ended_at: Option<Timestamp>,
    /// A relay region code, a version, or an error code; see [`valid_incident_detail`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Incident details are short identifiers: a DERP region (`fra`), a version
/// (`0.3.0`), or an error code (`ERROR_UPLOAD`). Nothing else fits.
pub fn valid_incident_detail(detail: &str) -> bool {
    (1..=24).contains(&detail.len())
        && detail
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

/// Distribution of one metric over a period.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct HealthStat {
    /// Samples.
    pub count: u32,
    /// Sum (the total, for counters).
    pub sum: f64,
    /// Smallest.
    pub min: f64,
    /// Median, from the histogram (about ±9 %).
    pub p50: f64,
    /// 95th percentile, from the histogram (about ±9 %).
    pub p95: f64,
    /// Largest.
    pub max: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(metric: HealthMetric, label: Option<HealthLabel>, value: f64) -> HealthSample {
        HealthSample {
            machine_id: MachineId("m".into()),
            metric,
            label,
            at: Timestamp(1_000),
            value,
        }
    }

    #[test]
    fn labels_are_required_exactly_where_they_belong() {
        let now = Timestamp(1_000);
        assert!(sample(HealthMetric::TypingEchoMs, None, 12.0)
            .validate(now)
            .is_ok());
        assert!(sample(
            HealthMetric::TypingEchoMs,
            Some(HealthLabel::LoadLive),
            12.0
        )
        .validate(now)
        .is_err());
        assert!(
            sample(HealthMetric::RouteMs, Some(HealthLabel::RoutePing), 3.0)
                .validate(now)
                .is_ok()
        );
        assert!(
            sample(HealthMetric::RouteMs, Some(HealthLabel::ErrorPty), 3.0)
                .validate(now)
                .is_err()
        );
        assert!(sample(HealthMetric::Errors, None, 1.0)
            .validate(now)
            .is_err());
        assert!(
            sample(HealthMetric::Errors, Some(HealthLabel::ErrorAttach), 1.0)
                .validate(now)
                .is_ok()
        );
    }

    #[test]
    fn values_and_times_are_bounded() {
        let now = Timestamp(1_000);
        for bad in [-1.0, f64::NAN, f64::INFINITY, 10_001.0] {
            assert!(
                sample(HealthMetric::TypingEchoMs, None, bad)
                    .validate(now)
                    .is_err(),
                "{bad}"
            );
        }
        let mut late = sample(HealthMetric::TypingEchoMs, None, 5.0);
        late.at = Timestamp(1_000 + MAX_SAMPLE_SKEW_MS + 1);
        assert_eq!(late.validate(now), Err(HealthSampleError::Time));
    }

    #[test]
    fn incident_details_are_short_identifiers_only() {
        for good in ["fra", "0.3.0", "ERROR_UPLOAD", "nyc-2"] {
            assert!(valid_incident_detail(good), "{good}");
        }
        for bad in [
            "",
            "/home/me",
            "100.64.0.1:7420",
            "two words",
            &"x".repeat(25),
        ] {
            assert!(!valid_incident_detail(bad), "{bad}");
        }
    }
}
