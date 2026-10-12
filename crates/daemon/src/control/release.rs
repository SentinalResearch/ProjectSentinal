//! The control's owner-signed releases for the dashboard (4.E §11b). The
//! `sentinal release` CLI does all verification and installation; this
//! service reads its owner-only state files, runs it for check-now and
//! policy, and starts it detached for an update so the update survives this
//! daemon restarting.

use super::ControlFailure;
use protocol::{
    link::LinkFailureCode,
    release::{
        strict_version, AvailableRelease, ControlReleaseView, ControlUpdateRequest,
        ControlUpdateStarted, ControlUpdateState, ControlUpdateStatus, ReleaseChannel,
        ReleasePolicy, ReleasePolicyRequest,
    },
    Timestamp,
};
use serde::Deserialize;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const WATCH_FILE: &str = "release-watch.json";
const POLICY_FILE: &str = "release-policy.json";
const UPDATE_FILE: &str = "release-update.json";
const UPDATE_LOG: &str = "release-update.log";
const MAX_STATE_BYTES: u64 = 64 * 1024;
/// An update without a recorded result after this long is shown as UNKNOWN.
const UPDATE_RESULT_DEADLINE_MS: i64 = 15 * 60 * 1000;
const CLI_TIMEOUT: Duration = Duration::from_secs(120);

/// How the detached updater is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detach {
    /// `systemd-run --user` (Linux): its own unit, outside sentinald's cgroup.
    SystemdRun,
    /// A new process group (macOS launchd; tests).
    ProcessGroup,
}

/// Wiring for one control's release service.
#[derive(Clone)]
pub struct ControlReleaseSettings {
    /// Daemon data directory holding the CLI's state files.
    pub data_dir: PathBuf,
    /// Daemon config directory passed to the CLI.
    pub config_dir: PathBuf,
    /// Install prefix (for `share/sentinal/release.json`).
    pub prefix: PathBuf,
    /// The installed `sentinal` binary.
    pub cli: PathBuf,
    /// How to start the detached updater.
    pub detach: Detach,
    /// Live FULL Sessions an update would interrupt.
    pub live_full_sessions: Arc<dyn Fn() -> u32 + Send + Sync>,
}

/// Owner-facing release state and actions for this control.
#[derive(Clone)]
pub struct ControlRelease {
    settings: ControlReleaseSettings,
    starting: Arc<Mutex<()>>,
}

#[derive(Debug, Default, Deserialize)]
struct WatchFile {
    checked_at: Option<i64>,
    current: Option<String>,
    available: Option<WatchAvailable>,
    unsigned_found: Option<String>,
    last_result: Option<WatchResult>,
}

#[derive(Debug, Deserialize)]
struct WatchAvailable {
    version: String,
    signed: bool,
    published_at: String,
    notes_url: String,
}

#[derive(Debug, Deserialize)]
struct WatchResult {
    state: String,
    version: String,
    previous: String,
    reason: Option<String>,
    at: i64,
}

#[derive(Debug, Deserialize)]
struct PolicyFile {
    policy: ReleasePolicy,
    channel: String,
    window: String,
}

#[derive(Debug, serde::Serialize, Deserialize)]
struct UpdateFile {
    update_id: String,
    version: String,
    previous: String,
    started_at: i64,
}

impl ControlRelease {
    /// A release service for one control.
    pub fn new(settings: ControlReleaseSettings) -> Self {
        Self {
            settings,
            starting: Arc::new(Mutex::new(())),
        }
    }

    /// The current release state; reads files only.
    pub fn view(&self) -> ControlReleaseView {
        let data = &self.settings.data_dir;
        let watch: WatchFile = read_json(&data.join(WATCH_FILE)).unwrap_or_default();
        let policy: Option<PolicyFile> = read_json(&data.join(POLICY_FILE));
        let current = installed_version(&self.settings.prefix)
            .or_else(|| watch.current.clone())
            .unwrap_or_else(|| crate::DAEMON_VERSION.to_string());
        let available = watch
            .available
            .as_ref()
            .filter(|offer| {
                offer.signed && strict_version(&offer.version) && newer(&offer.version, &current)
            })
            .map(|offer| AvailableRelease {
                version: offer.version.clone(),
                published_at: offer.published_at.clone(),
                notes_url: offer.notes_url.clone(),
            });
        ControlReleaseView {
            current,
            channel: match policy.as_ref().map(|file| file.channel.as_str()) {
                Some("beta") => ReleaseChannel::Beta,
                _ => ReleaseChannel::Stable,
            },
            policy: policy
                .as_ref()
                .map_or(ReleasePolicy::Notify, |file| file.policy),
            window: policy.map_or_else(|| "03:00-05:00".into(), |file| file.window),
            available,
            unsigned_found: watch
                .unsigned_found
                .clone()
                .filter(|version| strict_version(version)),
            last_checked_at: watch.checked_at.map(Timestamp),
            update: self.update_status(&watch),
            live_full_sessions: (self.settings.live_full_sessions)(),
        }
    }

    fn update_status(&self, watch: &WatchFile) -> Option<ControlUpdateStatus> {
        let started: Option<UpdateFile> = read_json(&self.settings.data_dir.join(UPDATE_FILE));
        let result = watch.last_result.as_ref();
        match started {
            Some(update) => {
                let finished = result.filter(|result| {
                    result.version == update.version && result.at >= update.started_at
                });
                let state = match finished {
                    Some(result) => result_state(&result.state),
                    None if now_ms() - update.started_at > UPDATE_RESULT_DEADLINE_MS => {
                        ControlUpdateState::Unknown
                    }
                    None => ControlUpdateState::Running,
                };
                Some(ControlUpdateStatus {
                    update_id: update.update_id,
                    version: update.version,
                    previous: update.previous,
                    state,
                    reason: finished.and_then(|result| result.reason.clone()),
                    started_at: Some(Timestamp(update.started_at)),
                    finished_at: finished.map(|result| Timestamp(result.at)),
                })
            }
            // An update run from a terminal: show its result.
            None => result.map(|result| ControlUpdateStatus {
                update_id: "cli".into(),
                version: result.version.clone(),
                previous: result.previous.clone(),
                state: result_state(&result.state),
                reason: result.reason.clone(),
                started_at: None,
                finished_at: Some(Timestamp(result.at)),
            }),
        }
    }

    /// Run `sentinal release check` now and return the refreshed state.
    pub async fn check(&self) -> Result<ControlReleaseView, ControlFailure> {
        self.run_cli(&["release", "check", "--json"]).await?;
        Ok(self.view())
    }

    /// Store a new policy and channel through the CLI.
    pub async fn set_policy(
        &self,
        request: ReleasePolicyRequest,
    ) -> Result<ControlReleaseView, ControlFailure> {
        let policy = match request.policy {
            ReleasePolicy::Notify => "notify",
            ReleasePolicy::AutomaticWhenIdle => "auto",
            ReleasePolicy::Off => "off",
        };
        let channel = match request.channel {
            ReleaseChannel::Stable => "stable",
            ReleaseChannel::Beta => "beta",
        };
        self.run_cli(&["release", "policy", policy, "--channel", channel])
            .await?;
        Ok(self.view())
    }

    /// Start a detached update to the offered signed release.
    pub fn start_update(
        &self,
        request: &ControlUpdateRequest,
    ) -> Result<ControlUpdateStarted, ControlFailure> {
        request.validate().map_err(|error| {
            ControlFailure::new(400, LinkFailureCode::InvalidRequest, error.to_string())
        })?;
        let _guard = self.starting.lock().map_err(|_| {
            ControlFailure::new(
                500,
                LinkFailureCode::Internal,
                "release state lock poisoned",
            )
        })?;
        let view = self.view();
        if view
            .update
            .as_ref()
            .is_some_and(|update| update.state == ControlUpdateState::Running)
        {
            return Err(conflict("a control update is already running"));
        }
        match &view.available {
            Some(offer) if offer.version == request.version => {}
            Some(offer) => {
                return Err(conflict(format!(
                    "{} is not the offered release; the newest signed release is {}",
                    request.version, offer.version
                )))
            }
            None => {
                return Err(conflict(
                    "no newer signed release is offered; check for updates first",
                ))
            }
        }
        if view.live_full_sessions > 0 && !request.allow_interrupt {
            let count = view.live_full_sessions;
            return Err(conflict(format!(
                "{count} live FULL Session{} would be interrupted; confirm to update anyway",
                if count == 1 { "" } else { "s" }
            )));
        }
        self.launch(&request.version, &view.current, Some(now_ms()))
    }

    /// Start the detached `sentinal release update` for `version`.
    pub(crate) fn launch(
        &self,
        version: &str,
        previous: &str,
        started_at: Option<i64>,
    ) -> Result<ControlUpdateStarted, ControlFailure> {
        let started_at = started_at.unwrap_or_else(now_ms);
        let update_id = format!("cu_{started_at}");
        let record = UpdateFile {
            update_id: update_id.clone(),
            version: version.into(),
            previous: previous.into(),
            started_at,
        };
        write_private(&self.settings.data_dir.join(UPDATE_FILE), &record)
            .map_err(|error| ControlFailure::new(500, LinkFailureCode::Internal, error))?;
        let log = private_log(&self.settings.data_dir.join(UPDATE_LOG))
            .map_err(|error| ControlFailure::new(500, LinkFailureCode::Internal, error))?;
        let config_dir = self.settings.config_dir.to_string_lossy().into_owned();
        let args = [
            "release",
            "update",
            "--version",
            version,
            "--yes",
            "--config-dir",
            config_dir.as_str(),
        ];
        let mut command = match self.settings.detach {
            Detach::SystemdRun => {
                let mut command = std::process::Command::new("systemd-run");
                command
                    .args(["--user", "--collect", "--quiet"])
                    .arg(format!("--unit=sentinal-control-update-{started_at}"))
                    .arg(&self.settings.cli)
                    .args(args);
                command
            }
            Detach::ProcessGroup => {
                let mut command = std::process::Command::new(&self.settings.cli);
                command.args(args);
                #[cfg(unix)]
                {
                    use std::os::unix::process::CommandExt;
                    command.process_group(0);
                }
                command
            }
        };
        let stderr = log.try_clone().map_err(|error| {
            ControlFailure::new(500, LinkFailureCode::Internal, error.to_string())
        })?;
        command.stdin(Stdio::null()).stdout(log).stderr(stderr);
        let mut child = command.spawn().map_err(|error| {
            ControlFailure::new(
                500,
                LinkFailureCode::Internal,
                format!("could not start the updater: {error}"),
            )
        })?;
        // Reap the child in the background so it never lingers as a zombie.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        tracing::info!(version, %update_id, "control update started");
        Ok(ControlUpdateStarted { update_id })
    }

    async fn run_cli(&self, args: &[&str]) -> Result<(), ControlFailure> {
        let mut command = tokio::process::Command::new(&self.settings.cli);
        command
            .args(args)
            .arg("--config-dir")
            .arg(&self.settings.config_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let output = tokio::time::timeout(CLI_TIMEOUT, command.output())
            .await
            .map_err(|_| {
                ControlFailure::new(
                    504,
                    LinkFailureCode::Internal,
                    "the release check timed out",
                )
            })?
            .map_err(|error| {
                ControlFailure::new(
                    500,
                    LinkFailureCode::Internal,
                    format!("could not run sentinal: {error}"),
                )
            })?;
        if output.status.success() {
            return Ok(());
        }
        let message: String = String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .unwrap_or("sentinal release failed")
            .trim_start_matches("sentinal: ")
            .chars()
            .filter(|character| !character.is_control())
            .take(300)
            .collect();
        Err(ControlFailure::new(502, LinkFailureCode::Internal, message))
    }
}

fn conflict(message: impl Into<String>) -> ControlFailure {
    ControlFailure::new(409, LinkFailureCode::InvalidRequest, message)
}

fn result_state(state: &str) -> ControlUpdateState {
    match state {
        "UPDATED" => ControlUpdateState::Updated,
        "ROLLED_BACK" => ControlUpdateState::RolledBack,
        _ => ControlUpdateState::Failed,
    }
}

fn installed_version(prefix: &Path) -> Option<String> {
    let value: serde_json::Value = read_json(&prefix.join("share/sentinal/release.json"))?;
    value
        .get("version")?
        .as_str()
        .filter(|version| strict_version(version))
        .map(str::to_owned)
}

fn newer(candidate: &str, current: &str) -> bool {
    let parse = |text: &str| -> Option<(u64, u64, u64)> {
        let mut parts = text.split('.').map(|part| part.parse::<u64>().ok());
        Some((parts.next()??, parts.next()??, parts.next()??))
    };
    match (parse(candidate), parse(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_STATE_BYTES {
        return None;
    }
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

fn write_private<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|error| error.to_string())?;
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    let mut file = crate::private_fs::private_file_options()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)
        .map_err(|error| format!("could not write '{}': {error}", temporary.display()))?;
    std::io::Write::write_all(&mut file, &bytes).map_err(|error| error.to_string())?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("could not replace '{}': {error}", path.display()))
}

fn private_log(path: &Path) -> Result<fs::File, String> {
    crate::private_fs::private_file_options()
        .append(true)
        .create(true)
        .open(path)
        .map_err(|error| format!("could not open '{}': {error}", path.display()))
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
