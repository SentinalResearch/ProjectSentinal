//! A worker updating itself when its control asks (4.E §11). The request
//! carries only a version; the worker runs its own `sentinal update`, which
//! installs from the worker's pinned control origin and verifies everything
//! independently. Two owner-only files survive the daemon's restart:
//! `worker-update.json` (what was asked) and `worker-update-result.json`
//! (written by the updater when it finishes).

use crate::control::Detach;
use protocol::{
    link::{LinkFailure, LinkFailureCode},
    worker_update::{WorkerUpdateRequest, WorkerUpdateState, WorkerUpdateStatus},
    Timestamp,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
};

const RECORD_FILE: &str = "worker-update.json";
const RESULT_FILE: &str = "worker-update-result.json";
const LOG_FILE: &str = "worker-update.log";
const MAX_REASON_CHARS: usize = 200;

/// What a dashboard update asked for.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateRecord {
    version: String,
    previous: String,
    started_at: i64,
}

/// What the updater wrote when it finished.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateResult {
    /// Whether `sentinal update` succeeded.
    pub ok: bool,
    /// Why not (bounded).
    #[serde(default)]
    pub reason: Option<String>,
}

/// Wiring for this worker's updater.
#[derive(Clone)]
pub struct WorkerUpdater {
    /// Daemon data directory (owner-only).
    pub data_dir: PathBuf,
    /// The installed `sentinal` CLI.
    pub cli: PathBuf,
    /// How the updater outlives the service restart.
    pub detach: Detach,
    /// This daemon's version.
    pub daemon_version: String,
    /// Live FULL Sessions an update would interrupt.
    pub live_full_sessions: Arc<dyn Fn() -> u32 + Send + Sync>,
    /// Wall clock.
    pub clock: Arc<dyn Fn() -> Timestamp + Send + Sync>,
}

/// A refused update: HTTP status plus a secret-free body.
pub type Refusal = (u16, LinkFailure);

fn refusal(status: u16, code: LinkFailureCode, message: impl Into<String>) -> Refusal {
    (
        status,
        LinkFailure {
            code,
            message: message.into(),
        },
    )
}

impl WorkerUpdater {
    /// Start the detached updater, unless live FULL Sessions need consent.
    pub fn start(&self, request: &WorkerUpdateRequest) -> Result<WorkerUpdateStatus, Refusal> {
        let now = (self.clock)();
        request
            .validate()
            .map_err(|error| refusal(400, LinkFailureCode::InvalidRequest, error.to_string()))?;
        if self.running(now) {
            return Err(refusal(
                409,
                LinkFailureCode::InvalidRequest,
                "an update is already running on this Machine",
            ));
        }
        let live = (self.live_full_sessions)();
        if live > 0 && !request.allow_interrupt {
            return Err(refusal(
                409,
                LinkFailureCode::LiveSessions,
                format!(
                    "{live} live FULL Session{} would be interrupted; confirm to update anyway",
                    if live == 1 { "" } else { "s" }
                ),
            ));
        }
        let internal =
            |error: std::io::Error| refusal(500, LinkFailureCode::Internal, error.to_string());
        let _ = std::fs::remove_file(self.data_dir.join(RESULT_FILE));
        let record = UpdateRecord {
            version: request.version.clone(),
            previous: self.daemon_version.clone(),
            started_at: now.0,
        };
        write_private(&self.data_dir.join(RECORD_FILE), &record).map_err(internal)?;
        let log = private_file(&self.data_dir.join(LOG_FILE)).map_err(internal)?;
        let result_file = self
            .data_dir
            .join(RESULT_FILE)
            .to_string_lossy()
            .into_owned();
        let args = ["update", "--result-file", result_file.as_str()];
        let mut command = match self.detach {
            Detach::SystemdRun => {
                let mut command = std::process::Command::new("systemd-run");
                command
                    .args(["--user", "--collect", "--quiet"])
                    .arg(format!("--unit=sentinal-worker-update-{}", now.0))
                    .arg(&self.cli)
                    .args(args);
                command
            }
            Detach::ProcessGroup => {
                let mut command = std::process::Command::new(&self.cli);
                command.args(args);
                use std::os::unix::process::CommandExt;
                command.process_group(0);
                command
            }
        };
        let stderr = log.try_clone().map_err(internal)?;
        command.stdin(Stdio::null()).stdout(log).stderr(stderr);
        let mut child = command.spawn().map_err(internal)?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        tracing::info!(version = %request.version, "worker update started");
        Ok(WorkerUpdateStatus {
            version: record.version,
            previous: record.previous,
            state: WorkerUpdateState::Started,
            reason: None,
            started_at: now,
        })
    }

    /// The latest update as this daemon sees it.
    pub fn status(&self) -> Option<WorkerUpdateStatus> {
        status_in(&self.data_dir, &self.daemon_version)
    }

    fn running(&self, now: Timestamp) -> bool {
        // A started update with no result and no new release, for at most 10 minutes.
        self.status().is_some_and(|status| {
            status.state == WorkerUpdateState::Started
                && now.0 - status.started_at.0 < 10 * 60 * 1000
        })
    }
}

/// Derive the status from the two files and the running version.
pub fn status_in(data_dir: &Path, running_version: &str) -> Option<WorkerUpdateStatus> {
    let record: UpdateRecord = read_json(&data_dir.join(RECORD_FILE))?;
    let result: Option<UpdateResult> = read_json(&data_dir.join(RESULT_FILE));
    let (state, reason) = match &result {
        Some(UpdateResult { ok: false, reason }) => (
            WorkerUpdateState::Failed,
            Some(bounded(reason.as_deref().unwrap_or("the updater failed"))),
        ),
        _ if running_version != record.previous => (WorkerUpdateState::Installed, None),
        Some(_) => (
            WorkerUpdateState::Failed,
            Some("the update finished but this Machine still runs the previous release".into()),
        ),
        None => (WorkerUpdateState::Started, None),
    };
    Some(WorkerUpdateStatus {
        version: if state == WorkerUpdateState::Installed {
            running_version.to_string()
        } else {
            record.version
        },
        previous: record.previous,
        state,
        reason,
        started_at: Timestamp(record.started_at),
    })
}

/// Written by `sentinal update --result-file` when it finishes.
pub fn write_result(path: &Path, result: &UpdateResult) -> std::io::Result<()> {
    write_private(path, result)
}

fn bounded(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_REASON_CHARS)
        .collect()
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    if text.len() > 4096 {
        return None;
    }
    serde_json::from_str(&text).ok()
}

fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    crate::private_fs::private_file_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)
}

fn write_private<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    use std::io::Write;
    let temporary = path.with_extension("json.tmp");
    let mut file = private_file(&temporary)?;
    file.write_all(&serde_json::to_vec(value).map_err(std::io::Error::other)?)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

#[cfg(test)]
mod tests {
    use super::{
        status_in, write_private, write_result, UpdateRecord, UpdateResult, RECORD_FILE,
        RESULT_FILE,
    };
    use protocol::worker_update::WorkerUpdateState;

    fn dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sentinal-wus-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn record(dir: &std::path::Path) {
        let record = UpdateRecord {
            version: "0.1.19".into(),
            previous: "0.1.18".into(),
            started_at: 5,
        };
        write_private(&dir.join(RECORD_FILE), &record).unwrap();
    }

    #[test]
    fn status_follows_the_record_the_result_and_the_running_version() {
        let dir = dir("states");
        assert!(status_in(&dir, "0.1.18").is_none(), "no update asked");
        record(&dir);
        assert_eq!(
            status_in(&dir, "0.1.18").unwrap().state,
            WorkerUpdateState::Started
        );
        let installed = status_in(&dir, "0.1.19").unwrap();
        assert_eq!(
            installed.state,
            WorkerUpdateState::Installed,
            "the restarted daemon runs the new release"
        );
        assert_eq!(installed.version, "0.1.19");

        write_result(
            &dir.join(RESULT_FILE),
            &UpdateResult {
                ok: true,
                reason: None,
            },
        )
        .unwrap();
        let unchanged = status_in(&dir, "0.1.18").unwrap();
        assert_eq!(unchanged.state, WorkerUpdateState::Failed);

        let reason = format!("bad\u{1b}[31m{}", "x".repeat(500));
        write_result(
            &dir.join(RESULT_FILE),
            &UpdateResult {
                ok: false,
                reason: Some(reason),
            },
        )
        .unwrap();
        let failed = status_in(&dir, "0.1.19").unwrap();
        assert_eq!(
            failed.state,
            WorkerUpdateState::Failed,
            "a reported failure wins"
        );
        let text = failed.reason.unwrap();
        assert!(text.chars().count() <= 200 && !text.contains('\u{1b}'));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
