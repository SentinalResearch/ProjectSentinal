//! A computer running the terminal-agent daemon.

use crate::{MachineId, MachineRole, Timestamp};
use serde::{Deserialize, Serialize};

/// Operating system family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Os {
    /// Any Linux distribution.
    Linux,
    /// macOS.
    Macos,
}

/// Whether the machine's daemon is currently reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MachineStatus {
    /// Daemon answered recently.
    Online,
    /// Daemon has not answered; last known data may be stale.
    Offline,
}

/// One GPU in a machine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Gpu {
    /// Marketing name, e.g. `"NVIDIA GeForce RTX 4090"`.
    pub name: String,
    /// Total video memory in bytes, if known.
    pub vram_bytes: Option<u64>,
}

/// A computer running the daemon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Machine {
    /// Stable ID, e.g. `"rtx-pc"`.
    pub id: MachineId,
    /// Human-friendly name, e.g. `"RTX-PC"`.
    pub name: String,
    /// Operating system family.
    pub os: Os,
    /// CPU model name.
    pub cpu: String,
    /// Logical CPU cores.
    pub cpu_cores: u32,
    /// Total RAM in bytes.
    pub ram_bytes: u64,
    /// GPUs found on the machine (may be empty).
    pub gpus: Vec<Gpu>,
    /// Reachability.
    pub status: MachineStatus,
    /// Last time the daemon was heard from.
    pub last_seen: Timestamp,
    /// Enrollment role, when this daemon takes part in a Machine registry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<MachineRole>,
}

impl Machine {
    /// True if the machine has at least one GPU.
    pub fn has_gpu(&self) -> bool {
        !self.gpus.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_machine(gpus: Vec<Gpu>) -> Machine {
        Machine {
            id: "rtx-pc".into(),
            name: "RTX-PC".into(),
            os: Os::Linux,
            cpu: "AMD Ryzen 9".into(),
            cpu_cores: 16,
            ram_bytes: 64 * 1024 * 1024 * 1024,
            gpus,
            status: MachineStatus::Online,
            last_seen: Timestamp(0),
            role: None,
        }
    }

    #[test]
    fn machine_with_gpu_reports_has_gpu() {
        let gpu = Gpu {
            name: "RTX 4090".into(),
            vram_bytes: Some(24 * 1024 * 1024 * 1024),
        };
        assert!(test_machine(vec![gpu]).has_gpu());
    }

    #[test]
    fn machine_without_gpu_reports_no_gpu() {
        assert!(!test_machine(vec![]).has_gpu());
    }

    #[test]
    fn os_uses_screaming_snake_case_on_the_wire() {
        assert_eq!(serde_json::to_string(&Os::Macos).unwrap(), "\"MACOS\"");
    }
}
