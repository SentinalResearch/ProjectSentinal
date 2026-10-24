//! The control's side of 4.H health: store daemon reports (its own and each
//! worker's, under the authenticated Machine ID only) and note restarts.

use super::ControlService;
use protocol::{
    observability::{DaemonHealthReport, HealthMetric, HealthSample, IncidentKind},
    MachineId,
};
use std::collections::HashMap;

/// Last start count seen per Machine, to notice restarts.
pub(crate) type StartCounts = std::sync::Mutex<HashMap<MachineId, u32>>;

/// The samples a daemon report becomes. Invalid values are left out.
pub fn report_samples(machine_id: &MachineId, report: &DaemonHealthReport) -> Vec<HealthSample> {
    let sample = |metric, label, value: f64| HealthSample {
        machine_id: machine_id.clone(),
        metric,
        label,
        at: report.at,
        value,
    };
    #[allow(clippy::cast_precision_loss)]
    let mut samples = vec![
        sample(HealthMetric::DaemonRssBytes, None, report.rss_bytes as f64),
        sample(HealthMetric::DaemonCpuPercent, None, report.cpu_percent),
        sample(
            HealthMetric::LiveSessions,
            None,
            f64::from(report.live_sessions),
        ),
        sample(
            HealthMetric::LogDiskBytes,
            None,
            report.log_disk_bytes as f64,
        ),
    ];
    if let Some(delay) = report.upload_delay_ms {
        samples.push(sample(HealthMetric::UploadDelayMs, None, f64::from(delay)));
    }
    if report.upload_failures > 0 {
        samples.push(sample(
            HealthMetric::UploadFailures,
            None,
            f64::from(report.upload_failures),
        ));
    }
    for error in &report.errors {
        samples.push(sample(
            HealthMetric::Errors,
            Some(error.code),
            f64::from(error.count),
        ));
    }
    samples
        .into_iter()
        .filter(|sample| sample.validate(report.at).is_ok())
        .collect()
}

impl ControlService {
    /// Store one daemon report for `machine_id`, which the caller has
    /// authenticated. A higher start count than last seen is a restart.
    pub fn record_daemon_health(
        &self,
        machine_id: &MachineId,
        daemon_version: &str,
        report: &DaemonHealthReport,
    ) {
        let now = (self.clock)();
        if report.at.0.abs_diff(now.0) > protocol::observability::MAX_SAMPLE_SKEW_MS as u64 {
            return;
        }
        if let Err(error) = self
            .store
            .record_health_samples(&report_samples(machine_id, report))
        {
            tracing::warn!(%error, "failed to store a health report");
        }
        let restarted = self.health_starts.lock().ok().is_some_and(|mut seen| {
            let previous = seen.insert(machine_id.clone(), report.restarts);
            previous.is_some_and(|previous| report.restarts > previous)
        });
        if restarted {
            if let Err(error) = self.store.record_health_moment(
                machine_id,
                IncidentKind::Restarted,
                report.at,
                Some(daemon_version),
            ) {
                tracing::warn!(%error, "failed to record a restart");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::report_samples;
    use protocol::{
        observability::{DaemonHealthReport, HealthErrorCount, HealthLabel, HealthMetric},
        MachineId, Timestamp,
    };

    #[test]
    fn a_report_becomes_one_sample_per_measure() {
        let report = DaemonHealthReport {
            at: Timestamp(1_000),
            rss_bytes: 20_000_000,
            cpu_percent: 0.5,
            live_sessions: 2,
            log_disk_bytes: 4_096,
            uptime_ms: 60_000,
            restarts: 3,
            upload_delay_ms: Some(900),
            upload_failures: 0,
            errors: vec![HealthErrorCount {
                code: HealthLabel::ErrorPty,
                count: 2,
            }],
        };
        let samples = report_samples(&MachineId("w".into()), &report);
        let metrics: Vec<_> = samples.iter().map(|sample| sample.metric).collect();
        assert_eq!(
            metrics,
            [
                HealthMetric::DaemonRssBytes,
                HealthMetric::DaemonCpuPercent,
                HealthMetric::LiveSessions,
                HealthMetric::LogDiskBytes,
                HealthMetric::UploadDelayMs,
                HealthMetric::Errors,
            ]
        );
        assert!(samples.iter().all(|sample| sample.machine_id.0 == "w"));
    }
}
