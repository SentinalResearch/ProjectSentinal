//! Health views for the owner (milestone 4.H §2.1): summary, incidents, and
//! the shareable report. Built from the control's health tables and registry.

use super::{ControlFailure, ControlService};
use protocol::{
    health_report::{
        HealthIncidents, HealthRange, HealthReport, HealthSummary, MachineHealth, MetricSeries,
        MetricStat, ReportCoverage, ReportIncident, ReportMachine, ReportNames, SeriesPoint,
        CHARTED_METRICS, HEALTH_REPORT_VERSION,
    },
    link::{MachineRole, RegistryMachine},
    observability::HealthMetric,
    MachineId, Timestamp,
};
use std::collections::HashMap;
use store::health::HealthStatRow;

/// "Latest" means the last five minutes.
const LATEST_MS: i64 = 5 * 60_000;
/// Include the minute in progress.
const MINUTE_MS: i64 = 60_000;

fn metric_stat(row: &HealthStatRow) -> MetricStat {
    MetricStat {
        metric: row.metric,
        label: row.label,
        stat: row.stat.clone(),
        dropped: row.dropped,
    }
}

fn by_machine(rows: Vec<HealthStatRow>) -> HashMap<MachineId, Vec<HealthStatRow>> {
    let mut grouped: HashMap<MachineId, Vec<HealthStatRow>> = HashMap::new();
    for row in rows {
        grouped.entry(row.machine_id.clone()).or_default().push(row);
    }
    grouped
}

impl ControlService {
    /// Registry rows, control first, revoked Machines left out.
    fn health_machines(&self) -> Result<Vec<RegistryMachine>, ControlFailure> {
        let mut machines: Vec<_> = self
            .registry()?
            .machines
            .into_iter()
            .filter(|machine| machine.status != protocol::link::RegistryStatus::Revoked)
            .collect();
        machines.sort_by_key(|machine| machine.role != MachineRole::Control);
        Ok(machines)
    }

    fn range_bounds(&self, range: HealthRange) -> (Timestamp, Timestamp, Timestamp) {
        let now = (self.clock)();
        let to = Timestamp(now.0 + MINUTE_MS);
        (now, Timestamp(now.0 - range.millis()), to)
    }

    /// `GET /api/v1/health/summary`.
    pub fn health_summary(&self, range: HealthRange) -> Result<HealthSummary, ControlFailure> {
        let (now, from, to) = self.range_bounds(range);
        let internal = ControlFailure::internal;
        let mut stats = by_machine(self.store.health_stats(from, to).map_err(internal)?);
        let mut latest = by_machine(
            self.store
                .health_stats(Timestamp(now.0 - LATEST_MS), to)
                .map_err(internal)?,
        );
        let mut machines = Vec::new();
        for machine in self.health_machines()? {
            let rows = stats.remove(&machine.machine_id).unwrap_or_default();
            let mut series = Vec::new();
            for metric in CHARTED_METRICS {
                if !rows.iter().any(|row| row.metric == metric) {
                    continue;
                }
                let points = self
                    .store
                    .health_series(&machine.machine_id, metric, None, from, to)
                    .map_err(internal)?
                    .into_iter()
                    .map(|point| SeriesPoint {
                        at: point.at,
                        count: point.stat.count,
                        p50: point.stat.p50,
                        p95: point.stat.p95,
                    })
                    .collect();
                series.push(MetricSeries { metric, points });
            }
            machines.push(MachineHealth {
                reports_health: rows
                    .iter()
                    .any(|row| row.metric == HealthMetric::DaemonRssBytes),
                latest: latest
                    .remove(&machine.machine_id)
                    .unwrap_or_default()
                    .iter()
                    .map(metric_stat)
                    .collect(),
                stats: rows.iter().map(metric_stat).collect(),
                series,
                machine_id: machine.machine_id,
                display_name: machine.display_name,
                role: machine.role,
                daemon_version: machine.daemon_version,
            });
        }
        Ok(HealthSummary {
            range,
            generated_at: now,
            storage_bytes: self.store.health_storage_bytes().map_err(internal)?,
            machines,
        })
    }

    /// `GET /api/v1/health/incidents`.
    pub fn health_incident_list(
        &self,
        range: HealthRange,
    ) -> Result<HealthIncidents, ControlFailure> {
        let (_, from, _) = self.range_bounds(range);
        Ok(HealthIncidents {
            incidents: self
                .store
                .health_incidents(from)
                .map_err(ControlFailure::internal)?,
        })
    }

    /// `GET /api/v1/health/report`. Anonymous reports name Machines
    /// "Machine 1", "Machine 2", … and carry no Machine ID.
    pub fn health_report(
        &self,
        range: HealthRange,
        names: ReportNames,
    ) -> Result<HealthReport, ControlFailure> {
        let (now, from, to) = self.range_bounds(range);
        let internal = ControlFailure::internal;
        let mut stats = by_machine(self.store.health_stats(from, to).map_err(internal)?);
        let mut labels = HashMap::new();
        let mut coverage = ReportCoverage {
            samples: 0,
            dropped: 0,
        };
        let mut machines = Vec::new();
        for (index, machine) in self.health_machines()?.into_iter().enumerate() {
            let name = match names {
                ReportNames::Anonymous => format!("Machine {}", index + 1),
                ReportNames::Real => machine.display_name.clone(),
            };
            labels.insert(machine.machine_id.clone(), name.clone());
            let rows = stats.remove(&machine.machine_id).unwrap_or_default();
            for row in &rows {
                coverage.samples += u64::from(row.stat.count);
                coverage.dropped += u64::from(row.dropped);
            }
            machines.push(ReportMachine {
                name,
                role: machine.role,
                os: machine.os,
                daemon_version: machine.daemon_version,
                stats: rows.iter().map(metric_stat).collect(),
            });
        }
        let incidents = self
            .store
            .health_incidents(from)
            .map_err(internal)?
            .into_iter()
            .filter_map(|incident| {
                Some(ReportIncident {
                    machine: labels.get(&incident.machine_id)?.clone(),
                    kind: incident.kind,
                    started_at: incident.started_at,
                    ended_at: incident.ended_at,
                    detail: incident.detail,
                })
            })
            .collect();
        Ok(HealthReport {
            schema_version: HEALTH_REPORT_VERSION,
            generated_at: now,
            range,
            names,
            machines,
            incidents,
            coverage,
        })
    }
}
