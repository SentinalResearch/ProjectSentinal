//! The daemon measuring itself once a minute (milestone 4.H §1.2): memory,
//! CPU, live Sessions, log disk use, uptime, restarts, upload health, and
//! error counts. Workers send the report in their heartbeat; the control
//! stores its own directly. Counts and sizes only.

use protocol::observability::{DaemonHealthReport, HealthErrorCount, HealthLabel};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
    time::Instant,
};

/// Error codes the daemon counts, in report order.
const ERROR_CODES: [HealthLabel; 5] = [
    HealthLabel::ErrorUpload,
    HealthLabel::ErrorHeartbeat,
    HealthLabel::ErrorLogWrite,
    HealthLabel::ErrorPty,
    HealthLabel::ErrorStore,
];
const NO_DELAY: u32 = u32::MAX;

/// Process-wide counters, cheap enough for any hot path (one atomic add).
pub struct HealthCounters {
    errors: [AtomicU32; ERROR_CODES.len()],
    upload_failures: AtomicU32,
    upload_delay_ms: AtomicU32,
}

impl HealthCounters {
    /// Zeroed counters.
    pub const fn new() -> Self {
        HealthCounters {
            errors: [const { AtomicU32::new(0) }; ERROR_CODES.len()],
            upload_failures: AtomicU32::new(0),
            upload_delay_ms: AtomicU32::new(NO_DELAY),
        }
    }

    /// Count one error; codes the daemon does not count are ignored.
    pub fn error(&self, code: HealthLabel) {
        if let Some(index) = ERROR_CODES.iter().position(|known| *known == code) {
            self.errors[index].fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Count one failed projection upload.
    pub fn upload_failed(&self) {
        self.upload_failures.fetch_add(1, Ordering::Relaxed);
        self.error(HealthLabel::ErrorUpload);
    }

    /// Record how old the oldest change was when an upload succeeded.
    pub fn upload_delay(&self, milliseconds: i64) {
        let value = u32::try_from(milliseconds.max(0)).unwrap_or(NO_DELAY - 1);
        self.upload_delay_ms.store(value, Ordering::Relaxed);
    }

    /// Take everything counted since the last call.
    fn take(&self) -> (Vec<HealthErrorCount>, u32, Option<u32>) {
        let errors = ERROR_CODES
            .iter()
            .zip(&self.errors)
            .filter_map(|(code, count)| {
                let count = count.swap(0, Ordering::Relaxed);
                (count > 0).then_some(HealthErrorCount { code: *code, count })
            })
            .collect();
        let failures = self.upload_failures.swap(0, Ordering::Relaxed);
        let delay = self.upload_delay_ms.swap(NO_DELAY, Ordering::Relaxed);
        (errors, failures, (delay != NO_DELAY).then_some(delay))
    }
}

impl Default for HealthCounters {
    fn default() -> Self {
        Self::new()
    }
}

/// This daemon's counters.
pub static COUNTERS: HealthCounters = HealthCounters::new();

/// Fold a report that could not be sent into the next one, so counts are
/// not lost when a heartbeat fails.
pub fn merge_unsent(
    unsent: Option<DaemonHealthReport>,
    mut next: DaemonHealthReport,
) -> DaemonHealthReport {
    let Some(unsent) = unsent else { return next };
    next.upload_failures = next.upload_failures.saturating_add(unsent.upload_failures);
    next.upload_delay_ms = next.upload_delay_ms.or(unsent.upload_delay_ms);
    for old in unsent.errors {
        match next.errors.iter_mut().find(|error| error.code == old.code) {
            Some(error) => error.count = error.count.saturating_add(old.count),
            None => next.errors.push(old),
        }
    }
    next
}

/// Count this start in `<data_dir>/health-starts` and return the total.
pub fn record_start(data_dir: &Path) -> u32 {
    let path = data_dir.join("health-starts");
    let starts = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
        .unwrap_or(0)
        .saturating_add(1);
    if let Err(error) = std::fs::write(&path, starts.to_string()) {
        tracing::debug!(%error, "could not record the daemon start count");
    }
    starts
}

/// Bytes of saved Session logs.
pub fn log_disk_bytes(data_dir: &Path) -> u64 {
    std::fs::read_dir(data_dir.join("sessions"))
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".log.zst"))
                .filter_map(|entry| entry.metadata().ok())
                .map(|metadata| metadata.len())
                .sum()
        })
        .unwrap_or(0)
}

/// Samples this process's memory and CPU (CPU is the average since the last sample).
pub struct SelfSampler {
    system: sysinfo::System,
    pid: sysinfo::Pid,
    started: Instant,
    restarts: u32,
    data_dir: PathBuf,
}

impl SelfSampler {
    /// A sampler for this process; `restarts` from [`record_start`].
    pub fn new(data_dir: PathBuf, restarts: u32) -> Self {
        let mut sampler = SelfSampler {
            system: sysinfo::System::new(),
            pid: sysinfo::Pid::from_u32(std::process::id()),
            started: Instant::now(),
            restarts,
            data_dir,
        };
        sampler.refresh();
        sampler
    }

    fn refresh(&mut self) {
        self.system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[self.pid]),
            true,
            sysinfo::ProcessRefreshKind::nothing()
                .with_memory()
                .with_cpu(),
        );
    }

    /// One report, taking the counters.
    pub fn sample(
        &mut self,
        at: protocol::Timestamp,
        live_sessions: u32,
        counters: &HealthCounters,
    ) -> DaemonHealthReport {
        self.refresh();
        let process = self.system.process(self.pid);
        let (errors, upload_failures, upload_delay_ms) = counters.take();
        DaemonHealthReport {
            at,
            rss_bytes: process.map_or(0, sysinfo::Process::memory),
            cpu_percent: process.map_or(0.0, |process| f64::from(process.cpu_usage())),
            live_sessions,
            log_disk_bytes: log_disk_bytes(&self.data_dir),
            uptime_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            restarts: self.restarts,
            upload_delay_ms,
            upload_failures,
            errors,
        }
    }
}

/// How often each daemon reports on itself.
pub const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// Sample once a minute. The control stores its own report; a worker queues
/// it for its next heartbeat.
pub fn spawn(
    data_dir: PathBuf,
    sessions: crate::session_manager::SessionManager,
    link: crate::link::LinkManager,
    control: Option<crate::control::ControlService>,
    clock: std::sync::Arc<dyn Fn() -> protocol::Timestamp + Send + Sync>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let restarts = record_start(&data_dir);
    tokio::spawn(async move {
        let mut sampler = SelfSampler::new(data_dir, restarts);
        let mut interval = tokio::time::interval(SAMPLE_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await; // the first tick fires at once; CPU needs a full interval
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = interval.tick() => {
                    let live = sessions.list().map_or(0, |list| {
                        list.iter().filter(|session| !session.state.is_finished()).count()
                    });
                    let report = sampler.sample(
                        clock(),
                        u32::try_from(live).unwrap_or(u32::MAX),
                        &COUNTERS,
                    );
                    match &control {
                        Some(control) => {
                            let id = control.machine_id().clone();
                            control.record_daemon_health(&id, crate::DAEMON_VERSION, &report);
                        }
                        None => link.set_health_report(report),
                    }
                }
            }
        }
    });
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use protocol::Timestamp;

    #[test]
    fn counters_are_taken_once() {
        let counters = HealthCounters::new();
        counters.error(HealthLabel::ErrorPty);
        counters.error(HealthLabel::ErrorPty);
        counters.upload_failed();
        counters.upload_delay(1_500);
        counters.error(HealthLabel::ErrorAttach); // a browser code: not counted here
        let (errors, failures, delay) = counters.take();
        assert_eq!(
            errors,
            vec![
                HealthErrorCount {
                    code: HealthLabel::ErrorUpload,
                    count: 1
                },
                HealthErrorCount {
                    code: HealthLabel::ErrorPty,
                    count: 2
                },
            ]
        );
        assert_eq!((failures, delay), (1, Some(1_500)));
        assert_eq!(counters.take(), (vec![], 0, None));
    }

    #[test]
    fn unsent_counts_carry_into_the_next_report() {
        let counters = HealthCounters::new();
        let dir = std::env::temp_dir().join(format!("sentinal-self-health-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sessions")).unwrap();
        std::fs::write(dir.join("sessions/1.log.zst"), [0u8; 100]).unwrap();
        std::fs::write(dir.join("sessions/notes.txt"), [0u8; 50]).unwrap();
        assert_eq!(record_start(&dir), 1);
        assert_eq!(record_start(&dir), 2);
        let mut sampler = SelfSampler::new(dir.clone(), 2);
        counters.upload_failed();
        let first = sampler.sample(Timestamp(1), 3, &counters);
        assert_eq!(
            (first.live_sessions, first.log_disk_bytes, first.restarts),
            (3, 100, 2)
        );
        assert!(first.rss_bytes > 0, "this process has resident memory");
        counters.error(HealthLabel::ErrorUpload);
        let second = merge_unsent(Some(first), sampler.sample(Timestamp(2), 3, &counters));
        assert_eq!(second.upload_failures, 1);
        assert_eq!(
            second.errors,
            vec![HealthErrorCount {
                code: HealthLabel::ErrorUpload,
                count: 2
            }]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
