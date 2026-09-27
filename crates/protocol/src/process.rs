//! Processes belonging to a session and their aggregate resource use.

use crate::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Current operating-system state of a process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProcessState {
    /// Scheduled or executing work.
    Running,
    /// Waiting for work or a resource.
    Sleeping,
    /// Suspended by a signal or debugger.
    Stopped,
    /// Exited but not yet reaped by its parent.
    Zombie,
    /// State could not be determined.
    Unknown,
}

/// One operating-system process observed under a session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Process {
    /// Operating-system process ID.
    pub pid: u32,
    /// Parent process ID, if known.
    pub parent_pid: Option<u32>,
    /// Executable name.
    pub name: String,
    /// Executable and its arguments.
    pub cmdline: Vec<String>,
    /// Current operating-system state.
    pub state: ProcessState,
    /// CPU usage as a percentage of one logical core.
    pub cpu_percent: f32,
    /// Resident memory in bytes.
    pub rss_bytes: u64,
    /// GPU memory in bytes, if available for this process.
    pub gpu_mem_bytes: Option<u64>,
    /// When the process started.
    pub started_at: Timestamp,
}

/// Aggregate resource use of a process tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProcessTotals {
    /// Sum of CPU percentages across the tree.
    pub cpu_percent: f32,
    /// Sum of resident memory in bytes across the tree.
    pub rss_bytes: u64,
    /// Sum of known GPU memory in bytes across the tree.
    pub gpu_mem_bytes: u64,
}

/// Sum resource use for `root` and every descendant, counting each PID once.
pub fn tree_totals(processes: &[Process], root: u32) -> ProcessTotals {
    let mut totals = ProcessTotals::default();
    let mut pending = vec![root];
    let mut visited = HashSet::new();

    while let Some(pid) = pending.pop() {
        if !visited.insert(pid) {
            continue;
        }
        let Some(process) = processes.iter().find(|process| process.pid == pid) else {
            continue;
        };

        totals.cpu_percent += process.cpu_percent;
        totals.rss_bytes = totals.rss_bytes.saturating_add(process.rss_bytes);
        totals.gpu_mem_bytes = totals
            .gpu_mem_bytes
            .saturating_add(process.gpu_mem_bytes.unwrap_or(0));

        pending.extend(
            processes
                .iter()
                .filter(|candidate| candidate.parent_pid == Some(pid))
                .map(|candidate| candidate.pid),
        );
    }

    totals
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, parent_pid: Option<u32>, cpu: f32, rss: u64, gpu: Option<u64>) -> Process {
        Process {
            pid,
            parent_pid,
            name: format!("process-{pid}"),
            cmdline: vec![format!("process-{pid}"), "--test".into()],
            state: ProcessState::Running,
            cpu_percent: cpu,
            rss_bytes: rss,
            gpu_mem_bytes: gpu,
            started_at: Timestamp(i64::from(pid)),
        }
    }

    #[test]
    fn tree_totals_include_root_and_nested_descendants() {
        let processes = vec![
            process(10, None, 1.5, 100, Some(10)),
            process(11, Some(10), 2.5, 200, None),
            process(12, Some(11), 3.0, 300, Some(30)),
            process(20, None, 99.0, 9_999, Some(9_999)),
        ];

        assert_eq!(
            tree_totals(&processes, 10),
            ProcessTotals {
                cpu_percent: 7.0,
                rss_bytes: 600,
                gpu_mem_bytes: 40,
            }
        );
    }

    #[test]
    fn tree_totals_for_unknown_root_are_zero() {
        let processes = vec![process(10, None, 1.5, 100, Some(10))];

        assert_eq!(tree_totals(&processes, 999), ProcessTotals::default());
    }

    #[test]
    fn tree_totals_count_each_pid_once_when_parent_links_cycle() {
        let processes = vec![
            process(10, Some(11), 1.0, 100, Some(10)),
            process(11, Some(10), 2.0, 200, Some(20)),
            process(12, Some(11), 3.0, 300, None),
        ];

        assert_eq!(
            tree_totals(&processes, 10),
            ProcessTotals {
                cpu_percent: 6.0,
                rss_bytes: 600,
                gpu_mem_bytes: 30,
            }
        );
    }
}
