//! Raw metric retention and one-minute downsampling.

use crate::{to_sql_id, Store, StoreError};
use protocol::{SessionId, Timestamp};
use rusqlite::params;

const RAW_RETENTION_MS: i64 = 60 * 60 * 1_000;
const MINUTE_MS: i64 = 60 * 1_000;

/// One raw machine or Session metric sample.
#[derive(Debug, Clone, PartialEq)]
pub struct RawMetricSample {
    /// Session scope, or `None` for the Machine.
    pub session_id: Option<SessionId>,
    /// Sample timestamp.
    pub at: Timestamp,
    /// CPU use percentage.
    pub cpu_percent: f32,
    /// Used RAM or resident bytes.
    pub ram_used_bytes: u64,
    /// Available RAM bytes, or zero for Session process trees.
    pub ram_total_bytes: u64,
    /// GPU utilization when available.
    pub gpu_percent: Option<f32>,
    /// Used VRAM when available.
    pub vram_used_bytes: Option<u64>,
    /// Total VRAM when available.
    pub vram_total_bytes: Option<u64>,
}

/// Averaged metric bucket aligned to one Unix minute.
#[derive(Debug, Clone, PartialEq)]
pub struct MinuteMetric {
    /// Session scope, or `None` for the Machine.
    pub session_id: Option<SessionId>,
    /// Start of the minute bucket.
    pub at: Timestamp,
    /// Average CPU percentage.
    pub cpu_percent: f32,
    /// Average used RAM bytes.
    pub ram_used_bytes: u64,
    /// Average total RAM bytes.
    pub ram_total_bytes: u64,
    /// Average GPU percentage when sampled.
    pub gpu_percent: Option<f32>,
    /// Average used VRAM bytes when sampled.
    pub vram_used_bytes: Option<u64>,
    /// Average total VRAM bytes when sampled.
    pub vram_total_bytes: Option<u64>,
    /// Number of raw samples represented.
    pub sample_count: u32,
}

impl Store {
    /// Insert a raw metric sample.
    pub fn insert_raw_metric(&self, sample: &RawMetricSample) -> Result<(), StoreError> {
        let session_id = sample.session_id.map(|id| to_sql_id(id.0)).transpose()?;
        self.lock()?.execute(
            "INSERT INTO metrics_raw(scope_key, session_id, at, cpu_percent, ram_used_bytes, ram_total_bytes,
             gpu_percent, vram_used_bytes, vram_total_bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                scope_key(sample.session_id),
                session_id,
                sample.at.0,
                sample.cpu_percent,
                to_sql_id(sample.ram_used_bytes)?,
                to_sql_id(sample.ram_total_bytes)?,
                sample.gpu_percent,
                sample.vram_used_bytes.map(to_sql_id).transpose()?,
                sample.vram_total_bytes.map(to_sql_id).transpose()?,
            ],
        )?;
        Ok(())
    }

    /// Downsample expired raw metrics and return the number removed.
    pub fn downsample_metrics(&self, now: Timestamp) -> Result<usize, StoreError> {
        let retention_cutoff = now.0.saturating_sub(RAW_RETENTION_MS);
        let cutoff = retention_cutoff.div_euclid(MINUTE_MS) * MINUTE_MS;
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO metrics_minute(scope_key, session_id, minute_at, cpu_percent, ram_used_bytes,
             ram_total_bytes, gpu_percent, vram_used_bytes, vram_total_bytes, sample_count)
             SELECT scope_key, session_id, (at / ?1) * ?1, AVG(cpu_percent), ROUND(AVG(ram_used_bytes)),
             ROUND(AVG(ram_total_bytes)), AVG(gpu_percent), ROUND(AVG(vram_used_bytes)),
             ROUND(AVG(vram_total_bytes)), COUNT(*) FROM metrics_raw WHERE at < ?2
             GROUP BY scope_key, (at / ?1) * ?1
             ON CONFLICT(scope_key, minute_at) DO UPDATE SET
             cpu_percent=excluded.cpu_percent, ram_used_bytes=excluded.ram_used_bytes,
             ram_total_bytes=excluded.ram_total_bytes, gpu_percent=excluded.gpu_percent,
             vram_used_bytes=excluded.vram_used_bytes, vram_total_bytes=excluded.vram_total_bytes,
             sample_count=excluded.sample_count",
            params![MINUTE_MS, cutoff],
        )?;
        let removed = transaction.execute("DELETE FROM metrics_raw WHERE at < ?1", [cutoff])?;
        transaction.commit()?;
        Ok(removed)
    }

    /// Return minute buckets ordered by time for diagnostics and tests.
    pub fn minute_metrics(&self) -> Result<Vec<MinuteMetric>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT session_id, minute_at, cpu_percent, ram_used_bytes, ram_total_bytes, gpu_percent,
             vram_used_bytes, vram_total_bytes, sample_count FROM metrics_minute ORDER BY minute_at, scope_key",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(MinuteMetric {
                session_id: row.get::<_, Option<u64>>(0)?.map(SessionId),
                at: Timestamp(row.get(1)?),
                cpu_percent: row.get(2)?,
                ram_used_bytes: row.get(3)?,
                ram_total_bytes: row.get(4)?,
                gpu_percent: row.get(5)?,
                vram_used_bytes: row.get(6)?,
                vram_total_bytes: row.get(7)?,
                sample_count: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Count raw metric samples retained in the one-hour window.
    pub fn raw_metric_count(&self) -> Result<u64, StoreError> {
        Ok(self
            .lock()?
            .query_row("SELECT COUNT(*) FROM metrics_raw", [], |row| row.get(0))?)
    }
}

fn scope_key(session_id: Option<SessionId>) -> String {
    session_id.map_or_else(|| "machine".into(), |id| format!("session:{}", id.0))
}
