//! Resource measurements for machines and sessions.

use crate::{ProcessTotals, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// Resource measurements for one GPU.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct GpuMetrics {
    /// Zero-based GPU index on the machine.
    pub index: u32,
    /// GPU compute utilization as a percentage.
    pub util_percent: f32,
    /// Video memory currently used in bytes.
    pub vram_used_bytes: u64,
    /// Total video memory in bytes.
    pub vram_total_bytes: u64,
    /// Temperature in degrees Celsius, if available.
    pub temp_c: Option<f32>,
}

impl GpuMetrics {
    /// Video memory usage as a percentage, or `None` when total memory is zero.
    pub fn vram_percent(&self) -> Option<f32> {
        percent(self.vram_used_bytes, self.vram_total_bytes)
    }
}

/// Resource measurements for a machine at one point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct MachineMetrics {
    /// When the measurements were taken.
    pub at: Timestamp,
    /// Total machine CPU utilization as a percentage.
    pub cpu_percent: f32,
    /// RAM currently used in bytes.
    pub ram_used_bytes: u64,
    /// Total RAM in bytes.
    pub ram_total_bytes: u64,
    /// Measurements for each detected GPU.
    pub gpus: Vec<GpuMetrics>,
}

impl MachineMetrics {
    /// RAM usage as a percentage, or `None` when total memory is zero.
    pub fn ram_percent(&self) -> Option<f32> {
        percent(self.ram_used_bytes, self.ram_total_bytes)
    }
}

/// Aggregate process-tree measurements for a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionMetrics {
    /// Session whose processes were measured.
    pub session_id: SessionId,
    /// When the measurements were taken.
    pub at: Timestamp,
    /// Aggregate resource use of the session's process tree.
    pub totals: ProcessTotals,
}

fn percent(used: u64, total: u64) -> Option<f32> {
    (total != 0).then(|| (used as f64 / total as f64 * 100.0) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ram_percent_uses_used_and_total_bytes() {
        let metrics = MachineMetrics {
            at: Timestamp(1),
            cpu_percent: 12.5,
            ram_used_bytes: 25,
            ram_total_bytes: 100,
            gpus: vec![],
        };

        assert_eq!(metrics.ram_percent(), Some(25.0));
    }

    #[test]
    fn ram_percent_is_none_when_total_is_zero() {
        let metrics = MachineMetrics {
            at: Timestamp(1),
            cpu_percent: 12.5,
            ram_used_bytes: 25,
            ram_total_bytes: 0,
            gpus: vec![],
        };

        assert_eq!(metrics.ram_percent(), None);
    }

    #[test]
    fn vram_percent_uses_used_and_total_bytes() {
        let metrics = GpuMetrics {
            index: 0,
            util_percent: 75.0,
            vram_used_bytes: 3,
            vram_total_bytes: 4,
            temp_c: Some(62.0),
        };

        assert_eq!(metrics.vram_percent(), Some(75.0));
    }

    #[test]
    fn vram_percent_is_none_when_total_is_zero() {
        let metrics = GpuMetrics {
            index: 0,
            util_percent: 0.0,
            vram_used_bytes: 0,
            vram_total_bytes: 0,
            temp_c: None,
        };

        assert_eq!(metrics.vram_percent(), None);
    }
}
