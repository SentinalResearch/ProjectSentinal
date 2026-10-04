//! Facts for the Machine page: host and daemon details, the latest process
//! tree of each live Session, and Machine-wide metric history.

use protocol::{
    link::ConnectionReport, MachineDetails, MachineMetricPoint, MetricRange, Process,
    ProcessSummary, SessionId, SessionProcesses, Timestamp,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};
use store::{Store, StoreError};

/// Processes kept per Session; the page shows the busiest ones.
const MAX_PROCESSES: usize = 10;

/// Static host facts captured once at daemon start.
#[derive(Debug, Clone)]
pub struct HostFacts {
    /// Hostname as the operating system reports it.
    pub hostname: String,
    /// Operating system name and version.
    pub os_version: Option<String>,
    /// Kernel version.
    pub kernel_version: Option<String>,
    /// Address the daemon API is bound to.
    pub bind: String,
    /// When this daemon process started.
    pub started_at: Timestamp,
    /// Installed release version, or the crate version for a development build.
    pub daemon_version: String,
}

impl HostFacts {
    /// Read this host's facts.
    pub fn detect(bind: String, started_at: Timestamp) -> Self {
        let os_version = sysinfo::System::long_os_version().or_else(|| {
            match (sysinfo::System::name(), sysinfo::System::os_version()) {
                (Some(name), Some(version)) => Some(format!("{name} {version}")),
                (name, version) => name.or(version),
            }
        });
        // The installer writes <prefix>/share/sentinal/release.json beside <prefix>/bin.
        let release = std::env::current_exe().ok().and_then(|exe| {
            let prefix = exe.parent()?.parent()?.to_path_buf();
            std::fs::read_to_string(prefix.join("share/sentinal/release.json")).ok()
        });
        HostFacts {
            daemon_version: release_version(release.as_deref()),
            hostname: sysinfo::System::host_name().unwrap_or_default(),
            os_version,
            kernel_version: sysinfo::System::kernel_version(),
            bind,
            started_at,
        }
    }
}

/// Shared Machine page state, cheap to clone.
#[derive(Clone)]
pub struct MachineInsights {
    host: HostFacts,
    store: Option<Arc<Store>>,
    processes: Arc<RwLock<BTreeMap<SessionId, Vec<ProcessSummary>>>>,
}

impl MachineInsights {
    /// Insights for this host, with history read from `store` when present.
    pub fn new(host: HostFacts, store: Option<Arc<Store>>) -> Self {
        MachineInsights {
            host,
            store,
            processes: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    /// Replace every Session's process tree with this sample's trees.
    pub fn record_processes(&self, trees: Vec<(SessionId, Vec<Process>)>) {
        let next = trees
            .into_iter()
            .map(|(session_id, tree)| (session_id, summarize(tree)))
            .collect();
        if let Ok(mut processes) = self.processes.write() {
            *processes = next;
        }
    }

    /// Details for the Machine page.
    pub fn details(&self, link: Option<ConnectionReport>) -> MachineDetails {
        let processes = self
            .processes
            .read()
            .map(|processes| {
                processes
                    .iter()
                    .map(|(session_id, processes)| SessionProcesses {
                        session_id: *session_id,
                        processes: processes.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        MachineDetails {
            hostname: self.host.hostname.clone(),
            os_version: self.host.os_version.clone(),
            kernel_version: self.host.kernel_version.clone(),
            daemon_version: self.host.daemon_version.clone(),
            started_at: self.host.started_at,
            bind: self.host.bind.clone(),
            link,
            processes,
        }
    }

    /// Machine-wide history over `range` ending at `now`.
    pub fn history(
        &self,
        range: MetricRange,
        now: Timestamp,
    ) -> Result<Vec<MachineMetricPoint>, StoreError> {
        match &self.store {
            Some(store) => store.machine_metric_points(
                Timestamp(now.0.saturating_sub(range.span_ms())),
                range.bucket_ms(),
            ),
            None => Ok(Vec::new()),
        }
    }
}

/// The `version` from installed release metadata, else a development label.
fn release_version(release_json: Option<&str>) -> String {
    release_json
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|value| value.get("version")?.as_str().map(str::to_owned))
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| format!("{} (development build)", env!("CARGO_PKG_VERSION")))
}

/// The busiest processes, without command lines (they may hold secrets).
fn summarize(mut tree: Vec<Process>) -> Vec<ProcessSummary> {
    tree.sort_by(|a, b| b.cpu_percent.total_cmp(&a.cpu_percent));
    tree.into_iter()
        .take(MAX_PROCESSES)
        .map(|process| ProcessSummary {
            pid: process.pid,
            parent_pid: process.parent_pid,
            name: process.name,
            cpu_percent: process.cpu_percent,
            rss_bytes: process.rss_bytes,
            gpu_mem_bytes: process.gpu_mem_bytes,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::ProcessState;
    use store::RawMetricSample;

    fn process(pid: u32, cpu: f32) -> Process {
        Process {
            pid,
            parent_pid: Some(1),
            name: format!("proc-{pid}"),
            cmdline: vec!["train".into(), "--token=secret-value".into()],
            state: ProcessState::Running,
            cpu_percent: cpu,
            rss_bytes: 1_024,
            gpu_mem_bytes: None,
            started_at: Timestamp(0),
        }
    }

    fn facts() -> HostFacts {
        HostFacts {
            hostname: "pop-os".into(),
            os_version: Some("Pop!_OS 22.04".into()),
            kernel_version: Some("6.9".into()),
            bind: "100.74.239.89:7420".into(),
            started_at: Timestamp(5),
            daemon_version: "0.1.10".into(),
        }
    }

    #[test]
    fn details_keep_the_busiest_processes_without_command_lines() {
        let insights = MachineInsights::new(facts(), None);
        let tree = (0..15).map(|pid| process(pid, pid as f32)).collect();
        insights.record_processes(vec![(SessionId(3), tree)]);

        let details = insights.details(None);

        assert_eq!(details.hostname, "pop-os");
        assert_eq!(details.daemon_version, "0.1.10");
        assert_eq!(details.processes.len(), 1);
        let listed = &details.processes[0].processes;
        assert_eq!(listed.len(), MAX_PROCESSES);
        assert_eq!(listed[0].pid, 14);
        let json = serde_json::to_string(&details).unwrap_or_default();
        assert!(!json.contains("secret-value"));
    }

    #[test]
    fn release_version_prefers_installed_metadata() {
        assert_eq!(
            release_version(Some(r#"{"schema_version":1,"version":"0.1.10"}"#)),
            "0.1.10"
        );
        let development = format!("{} (development build)", env!("CARGO_PKG_VERSION"));
        assert_eq!(release_version(None), development);
        assert_eq!(release_version(Some("not json")), development);
    }

    #[test]
    fn a_new_sample_drops_sessions_that_ended() {
        let insights = MachineInsights::new(facts(), None);
        insights.record_processes(vec![(SessionId(1), vec![process(1, 1.0)])]);
        insights.record_processes(vec![(SessionId(2), vec![process(2, 1.0)])]);
        let ids: Vec<_> = insights
            .details(None)
            .processes
            .iter()
            .map(|entry| entry.session_id)
            .collect();
        assert_eq!(ids, vec![SessionId(2)]);
    }

    #[test]
    fn history_reads_only_the_requested_range() {
        let store = Arc::new(Store::open_in_memory().unwrap_or_else(|_| unreachable!()));
        for at in [1_000, 10 * 60 * 60 * 1_000] {
            store
                .insert_raw_metric(&RawMetricSample {
                    session_id: None,
                    at: Timestamp(at),
                    cpu_percent: 5.0,
                    ram_used_bytes: 1,
                    ram_total_bytes: 2,
                    gpu_percent: None,
                    vram_used_bytes: None,
                    vram_total_bytes: None,
                })
                .unwrap_or_else(|_| unreachable!());
        }
        let insights = MachineInsights::new(facts(), Some(store));
        let hour = insights
            .history(MetricRange::Hour, Timestamp(10 * 60 * 60 * 1_000 + 5))
            .unwrap_or_default();
        assert_eq!(hour.len(), 1);
        let day = insights
            .history(MetricRange::Day, Timestamp(10 * 60 * 60 * 1_000 + 5))
            .unwrap_or_default();
        assert_eq!(day.len(), 2);
    }
}
