//! Bounded Session-log storage (F9). Finished Sessions' compressed logs are
//! deleted oldest first when they pass an age, or when all logs together pass
//! a byte budget. A live Session's log is never deleted (its own growth is
//! capped in `session_manager::log_budget`).

use protocol::SessionId;
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

/// Default total budget for all Session logs.
pub const DEFAULT_SESSION_LOG_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Default age after which a finished Session's log is deleted.
pub const DEFAULT_SESSION_LOG_MAX_AGE_DAYS: u64 = 90;

/// Retention limits from `[storage]` in `daemon.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageLimits {
    /// Total bytes all Session logs may use.
    pub session_log_max_bytes: u64,
    /// Days a finished Session's log is kept.
    pub session_log_max_age_days: u64,
}

impl Default for StorageLimits {
    fn default() -> Self {
        Self {
            session_log_max_bytes: DEFAULT_SESSION_LOG_MAX_BYTES,
            session_log_max_age_days: DEFAULT_SESSION_LOG_MAX_AGE_DAYS,
        }
    }
}

/// What one pass deleted.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PruneReport {
    /// Logs deleted.
    pub deleted: usize,
    /// Bytes freed.
    pub freed_bytes: u64,
    /// Bytes all Session logs use afterwards.
    pub remaining_bytes: u64,
}

struct LogFile {
    path: PathBuf,
    session_id: SessionId,
    bytes: u64,
    modified: SystemTime,
}

/// Apply `limits` to `<sessions_dir>/*.log.zst`; `is_live` protects running Sessions.
pub fn prune_session_logs(
    sessions_dir: &Path,
    limits: StorageLimits,
    is_live: impl Fn(SessionId) -> bool,
    now: SystemTime,
) -> io::Result<PruneReport> {
    let mut logs = Vec::new();
    let entries = match fs::read_dir(sessions_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(PruneReport::default()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let Some(session_id) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".log.zst"))
            .and_then(|id| id.parse::<u64>().ok())
            .map(SessionId)
        else {
            continue;
        };
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() {
            continue;
        }
        logs.push(LogFile {
            path,
            session_id,
            bytes: metadata.len(),
            modified: metadata.modified().unwrap_or(now),
        });
    }
    logs.sort_by_key(|log| log.modified);

    let max_age = Duration::from_secs(limits.session_log_max_age_days.saturating_mul(24 * 60 * 60));
    let mut total: u64 = logs.iter().map(|log| log.bytes).sum();
    let mut report = PruneReport::default();
    for log in &logs {
        if is_live(log.session_id) {
            continue;
        }
        let too_old = now
            .duration_since(log.modified)
            .is_ok_and(|age| age > max_age);
        if !too_old && total <= limits.session_log_max_bytes {
            continue;
        }
        fs::remove_file(&log.path)?;
        total = total.saturating_sub(log.bytes);
        report.deleted += 1;
        report.freed_bytes += log.bytes;
    }
    report.remaining_bytes = total;
    Ok(report)
}

/// Prune once at start and then daily; a Session is live unless the store
/// says it has finished (an unknown log is treated as finished).
pub fn spawn_storage_retention(
    data_dir: PathBuf,
    store: std::sync::Arc<store::Store>,
    limits: StorageLimits,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        loop {
            let sessions_dir = data_dir.join("sessions");
            let store = store.clone();
            let result = tokio::task::spawn_blocking(move || {
                prune_session_logs(
                    &sessions_dir,
                    limits,
                    |id| {
                        store
                            .session(id)
                            .ok()
                            .flatten()
                            .is_some_and(|session| !session.state.is_finished())
                    },
                    SystemTime::now(),
                )
            })
            .await;
            match result {
                Ok(Ok(report)) if report.deleted > 0 => tracing::info!(
                    deleted = report.deleted,
                    freed_bytes = report.freed_bytes,
                    remaining_bytes = report.remaining_bytes,
                    "pruned finished Session logs"
                ),
                Ok(Ok(_)) => {}
                Ok(Err(error)) => tracing::warn!(%error, "Session log retention failed"),
                Err(error) => tracing::warn!(%error, "Session log retention task failed"),
            }
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = tokio::time::sleep(Duration::from_secs(24 * 60 * 60)) => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{prune_session_logs, StorageLimits};
    use protocol::SessionId;
    use std::{
        fs::{self, File},
        path::Path,
        time::{Duration, SystemTime},
    };

    const DAY: Duration = Duration::from_secs(24 * 60 * 60);

    fn log(dir: &Path, id: u64, bytes: usize, age: Duration, now: SystemTime) {
        let path = dir.join(format!("{id}.log.zst"));
        fs::write(&path, vec![0_u8; bytes]).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(now - age)
            .unwrap();
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sentinal-storage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn old_finished_logs_go_but_live_ones_never_do() {
        let dir = dir("age");
        let now = SystemTime::now();
        log(&dir, 1, 10, 100 * DAY, now); // finished, too old
        log(&dir, 2, 10, 100 * DAY, now); // live, too old: kept
        log(&dir, 3, 10, DAY, now); // finished, recent
        fs::write(dir.join("notes.txt"), b"x").unwrap(); // not a Session log
        let limits = StorageLimits {
            session_log_max_bytes: 1_000,
            session_log_max_age_days: 90,
        };
        let report = prune_session_logs(&dir, limits, |id| id == SessionId(2), now).unwrap();
        assert_eq!(report.deleted, 1);
        assert!(!dir.join("1.log.zst").exists());
        assert!(dir.join("2.log.zst").exists() && dir.join("3.log.zst").exists());
        assert!(dir.join("notes.txt").exists());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn over_budget_deletes_the_oldest_finished_logs_first_until_within_it() {
        let dir = dir("budget");
        let now = SystemTime::now();
        log(&dir, 1, 40, 5 * DAY, now);
        log(&dir, 2, 40, 4 * DAY, now); // live
        log(&dir, 3, 40, 3 * DAY, now);
        log(&dir, 4, 40, 2 * DAY, now);
        let limits = StorageLimits {
            session_log_max_bytes: 100,
            session_log_max_age_days: 90,
        };
        let report = prune_session_logs(&dir, limits, |id| id == SessionId(2), now).unwrap();
        assert_eq!(report.deleted, 2, "{report:?}");
        assert!(!dir.join("1.log.zst").exists() && !dir.join("3.log.zst").exists());
        assert!(dir.join("2.log.zst").exists() && dir.join("4.log.zst").exists());
        assert_eq!(report.remaining_bytes, 80);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_sessions_directory_is_nothing_to_prune() {
        let report = prune_session_logs(
            Path::new("/nonexistent/sentinal-sessions"),
            StorageLimits::default(),
            |_| false,
            SystemTime::now(),
        )
        .unwrap();
        assert_eq!(report.deleted, 0);
    }
}
