//! Health samples, one-minute summaries, and incidents (milestone 4.H).
//!
//! Every sample is merged into its minute's histogram as it is stored, so a
//! sample that arrives late still lands in the right minute. Raw samples are
//! kept 24 h, mostly to enforce the per-minute cap; summaries and incidents
//! are kept 30 days.

use crate::{Store, StoreError};
use protocol::{
    observability::{
        HealthIncident, HealthLabel, HealthMetric, HealthSample, HealthStat, IncidentKind,
    },
    MachineId, Timestamp,
};
mod histogram;

pub use histogram::{bucket, Histogram};
use rusqlite::{params, OptionalExtension, Transaction};
use std::collections::BTreeMap;

const MINUTE_MS: i64 = 60_000;
/// Raw samples kept this long.
pub const RAW_RETENTION_MS: i64 = 24 * 60 * MINUTE_MS;
/// Summaries and incidents kept this long.
pub const SUMMARY_RETENTION_MS: i64 = 30 * 24 * 60 * MINUTE_MS;
/// Raw samples per Machine, metric, label, and minute; more are counted as dropped.
pub const MAX_SAMPLES_PER_MINUTE: i64 = 120;
/// Most incidents one listing returns.
pub const MAX_INCIDENTS: usize = 200;
/// Most points one series returns.
pub const MAX_SERIES_POINTS: usize = 300;

fn name<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(text.to_string())).ok()
}

fn label_name(label: Option<HealthLabel>) -> String {
    label.as_ref().map(name).unwrap_or_default()
}

/// One metric's summary for one Machine over a period.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthStatRow {
    /// Machine.
    pub machine_id: MachineId,
    /// Metric.
    pub metric: HealthMetric,
    /// Label, for labelled metrics.
    pub label: Option<HealthLabel>,
    /// Summary.
    pub stat: HealthStat,
    /// Samples refused by the cap in this period.
    pub dropped: u32,
}

/// One point of a chart series: the start of its period and the summary.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthPoint {
    /// Period start.
    pub at: Timestamp,
    /// Summary of the period.
    pub stat: HealthStat,
}

/// What `record_health_samples` did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HealthInsert {
    /// Stored samples.
    pub stored: usize,
    /// Samples over the per-minute cap (counted, not stored).
    pub dropped: usize,
}

fn merge_into_minute(
    transaction: &Transaction<'_>,
    key: (&str, &str, &str, i64),
    addition: &Histogram,
    dropped: u32,
) -> Result<(), StoreError> {
    let (machine, metric, label, minute) = key;
    let existing = transaction
        .query_row(
            "SELECT count, sum, min, max, buckets, dropped FROM health_minutes
             WHERE machine_id = ?1 AND metric = ?2 AND label = ?3 AND minute_at = ?4",
            params![machine, metric, label, minute],
            |row| {
                Ok((
                    Histogram::from_row(
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        &row.get::<_, String>(4)?,
                    ),
                    row.get::<_, u32>(5)?,
                ))
            },
        )
        .optional()?;
    let (mut histogram, previous_dropped) = existing.unwrap_or_default();
    histogram.merge(addition);
    transaction.execute(
        "INSERT INTO health_minutes(machine_id, metric, label, minute_at, count, sum, min, max, buckets, dropped)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(machine_id, metric, label, minute_at) DO UPDATE SET
           count = excluded.count, sum = excluded.sum, min = excluded.min, max = excluded.max,
           buckets = excluded.buckets, dropped = excluded.dropped",
        params![
            machine,
            metric,
            label,
            minute,
            histogram.count,
            histogram.sum,
            histogram.min,
            histogram.max,
            histogram.encode(),
            previous_dropped + dropped,
        ],
    )?;
    Ok(())
}

impl Store {
    /// Store validated samples in one transaction, honouring the per-minute cap.
    pub fn record_health_samples(
        &self,
        samples: &[HealthSample],
    ) -> Result<HealthInsert, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let mut result = HealthInsert::default();
        for sample in samples {
            let metric = name(&sample.metric);
            let label = label_name(sample.label);
            let minute = sample.at.0.div_euclid(MINUTE_MS) * MINUTE_MS;
            let key = (
                sample.machine_id.0.as_str(),
                metric.as_str(),
                label.as_str(),
                minute,
            );
            let stored: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM health_samples
                 WHERE machine_id = ?1 AND metric = ?2 AND label = ?3 AND minute_at = ?4",
                params![key.0, key.1, key.2, key.3],
                |row| row.get(0),
            )?;
            if stored >= MAX_SAMPLES_PER_MINUTE {
                merge_into_minute(&transaction, key, &Histogram::default(), 1)?;
                result.dropped += 1;
                continue;
            }
            transaction.execute(
                "INSERT INTO health_samples(machine_id, metric, label, minute_at, at, value)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![key.0, key.1, key.2, key.3, sample.at.0, sample.value],
            )?;
            let mut one = Histogram::default();
            one.add(sample.value);
            merge_into_minute(&transaction, key, &one, 0)?;
            result.stored += 1;
        }
        transaction.commit()?;
        Ok(result)
    }

    /// Summaries per Machine, metric, and label for minutes in `[from, to)`.
    pub fn health_stats(
        &self,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<HealthStatRow>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT machine_id, metric, label, count, sum, min, max, buckets, dropped FROM health_minutes
             WHERE minute_at >= ?1 AND minute_at < ?2 ORDER BY machine_id, metric, label",
        )?;
        let mut merged: BTreeMap<(String, String, String), (Histogram, u32)> = BTreeMap::new();
        let rows = statement.query_map(params![from.0, to.0], |row| {
            Ok((
                (
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ),
                Histogram::from_row(
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    &row.get::<_, String>(7)?,
                ),
                row.get::<_, u32>(8)?,
            ))
        })?;
        for row in rows {
            let (key, histogram, dropped) = row?;
            let entry = merged.entry(key).or_default();
            entry.0.merge(&histogram);
            entry.1 += dropped;
        }
        Ok(merged
            .into_iter()
            .filter_map(|((machine, metric, label), (histogram, dropped))| {
                Some(HealthStatRow {
                    machine_id: MachineId(machine),
                    metric: parse(&metric)?,
                    label: if label.is_empty() {
                        None
                    } else {
                        Some(parse(&label)?)
                    },
                    stat: histogram.stat().unwrap_or(HealthStat {
                        count: 0,
                        sum: 0.0,
                        min: 0.0,
                        p50: 0.0,
                        p95: 0.0,
                        max: 0.0,
                    }),
                    dropped,
                })
            })
            .collect())
    }

    /// A chart series for one Machine and metric over `[from, to)`, at most
    /// [`MAX_SERIES_POINTS`] points (minutes are merged into wider periods).
    pub fn health_series(
        &self,
        machine_id: &MachineId,
        metric: HealthMetric,
        label: Option<HealthLabel>,
        from: Timestamp,
        to: Timestamp,
    ) -> Result<Vec<HealthPoint>, StoreError> {
        let span = (to.0 - from.0).max(MINUTE_MS);
        let points = i64::try_from(MAX_SERIES_POINTS).unwrap_or(300);
        let width = ((span + points - 1) / points).div_euclid(MINUTE_MS).max(1) * MINUTE_MS;
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT minute_at, count, sum, min, max, buckets FROM health_minutes
             WHERE machine_id = ?1 AND metric = ?2 AND label = ?3 AND minute_at >= ?4 AND minute_at < ?5
             ORDER BY minute_at",
        )?;
        let mut periods: BTreeMap<i64, Histogram> = BTreeMap::new();
        let rows = statement.query_map(
            params![machine_id.0, name(&metric), label_name(label), from.0, to.0],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    Histogram::from_row(
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        &row.get::<_, String>(5)?,
                    ),
                ))
            },
        )?;
        for row in rows {
            let (minute, histogram) = row?;
            let start = from.0 + (minute - from.0).div_euclid(width) * width;
            periods.entry(start).or_default().merge(&histogram);
        }
        Ok(periods
            .into_iter()
            .filter_map(|(at, histogram)| {
                Some(HealthPoint {
                    at: Timestamp(at),
                    stat: histogram.stat()?,
                })
            })
            .collect())
    }

    /// Open an incident unless one of this kind is already open for the Machine.
    pub fn open_health_incident(
        &self,
        machine_id: &MachineId,
        kind: IncidentKind,
        at: Timestamp,
        detail: Option<&str>,
    ) -> Result<bool, StoreError> {
        let detail = detail.filter(|detail| protocol::observability::valid_incident_detail(detail));
        let connection = self.lock()?;
        let open: Option<i64> = connection
            .query_row(
                "SELECT id FROM health_incidents WHERE machine_id = ?1 AND kind = ?2 AND ended_at IS NULL",
                params![machine_id.0, name(&kind)],
                |row| row.get(0),
            )
            .optional()?;
        if open.is_some() {
            return Ok(false);
        }
        connection.execute(
            "INSERT INTO health_incidents(machine_id, kind, started_at, ended_at, detail) VALUES (?1, ?2, ?3, NULL, ?4)",
            params![machine_id.0, name(&kind), at.0, detail],
        )?;
        Ok(true)
    }

    /// Close the open incident of this kind, if any.
    pub fn close_health_incident(
        &self,
        machine_id: &MachineId,
        kind: IncidentKind,
        at: Timestamp,
    ) -> Result<bool, StoreError> {
        let changed = self.lock()?.execute(
            "UPDATE health_incidents SET ended_at = ?3 WHERE machine_id = ?1 AND kind = ?2 AND ended_at IS NULL",
            params![machine_id.0, name(&kind), at.0],
        )?;
        Ok(changed > 0)
    }

    /// Record a one-moment incident (a restart, a failed update).
    pub fn record_health_moment(
        &self,
        machine_id: &MachineId,
        kind: IncidentKind,
        at: Timestamp,
        detail: Option<&str>,
    ) -> Result<(), StoreError> {
        let detail = detail.filter(|detail| protocol::observability::valid_incident_detail(detail));
        self.lock()?.execute(
            "INSERT INTO health_incidents(machine_id, kind, started_at, ended_at, detail) VALUES (?1, ?2, ?3, ?3, ?4)",
            params![machine_id.0, name(&kind), at.0, detail],
        )?;
        Ok(())
    }

    /// Incidents that overlap `[from, ∞)`, newest first, at most [`MAX_INCIDENTS`].
    pub fn health_incidents(&self, from: Timestamp) -> Result<Vec<HealthIncident>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT machine_id, kind, started_at, ended_at, detail FROM health_incidents
             WHERE ended_at IS NULL OR ended_at >= ?1 ORDER BY started_at DESC, id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(
            params![from.0, i64::try_from(MAX_INCIDENTS).unwrap_or(200)],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            },
        )?;
        let mut incidents = Vec::new();
        for row in rows {
            let (machine, kind, started, ended, detail) = row?;
            if let Some(kind) = parse(&kind) {
                incidents.push(HealthIncident {
                    machine_id: MachineId(machine),
                    kind,
                    started_at: Timestamp(started),
                    ended_at: ended.map(Timestamp),
                    detail,
                });
            }
        }
        Ok(incidents)
    }

    /// Drop raw samples older than 24 h and summaries/incidents older than 30 days.
    pub fn prune_health(&self, now: Timestamp) -> Result<usize, StoreError> {
        let connection = self.lock()?;
        let raw = connection.execute(
            "DELETE FROM health_samples WHERE at < ?1",
            [now.0 - RAW_RETENTION_MS],
        )?;
        let minutes = connection.execute(
            "DELETE FROM health_minutes WHERE minute_at < ?1",
            [now.0 - SUMMARY_RETENTION_MS],
        )?;
        let incidents = connection.execute(
            "DELETE FROM health_incidents WHERE ended_at IS NOT NULL AND ended_at < ?1",
            [now.0 - SUMMARY_RETENTION_MS],
        )?;
        Ok(raw + minutes + incidents)
    }

    /// When the oldest projection change after `cursor` was queued, for the
    /// upload-delay measurement.
    pub fn oldest_projection_change_at(
        &self,
        machine_id: &MachineId,
        source_epoch: &str,
        cursor: u64,
    ) -> Result<Option<Timestamp>, StoreError> {
        let at: Option<i64> = self.lock()?.query_row(
            "SELECT MIN(created_at) FROM projection_outbox
             WHERE machine_id = ?1 AND source_epoch = ?2 AND cursor > ?3",
            params![machine_id.0, source_epoch, crate::to_sql_id(cursor)?],
            |row| row.get(0),
        )?;
        Ok(at.map(Timestamp))
    }

    /// Approximate bytes used by health data (rows × typical row size).
    pub fn health_storage_bytes(&self) -> Result<u64, StoreError> {
        let connection = self.lock()?;
        let bytes: i64 = connection.query_row(
            "SELECT (SELECT COUNT(*) FROM health_samples) * 64
                  + (SELECT COALESCE(SUM(96 + LENGTH(buckets)), 0) FROM health_minutes)
                  + (SELECT COUNT(*) FROM health_incidents) * 64",
            [],
            |row| row.get(0),
        )?;
        Ok(u64::try_from(bytes).unwrap_or(0))
    }
}
