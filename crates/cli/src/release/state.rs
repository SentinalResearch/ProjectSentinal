//! The control's release watch result and update policy, stored owner-only
//! (0600) in the daemon data directory and replaced atomically.

use super::{channel_name, github::CheckOutcome, now_ms, set_mode, Context, ReleaseError, Version};
use crate::release_args::{ReleaseChannel, ReleasePolicyArg};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const WATCH_FILE: &str = "release-watch.json";
const POLICY_FILE: &str = "release-policy.json";
const DEFAULT_WINDOW: &str = "03:00-05:00";

/// Stored update policy (read by the daemon's release watch).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum Policy {
    /// Notify and wait for the owner.
    Notify,
    /// Update when idle in the maintenance window.
    AutomaticWhenIdle,
    /// No automatic GitHub requests.
    Off,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PolicyFile {
    policy: Policy,
    channel: String,
    window: String,
}

/// The policy in effect.
#[derive(Debug, Clone)]
pub(crate) struct StoredPolicy {
    pub(crate) policy: Policy,
    pub(crate) channel: ReleaseChannel,
    pub(crate) window: String,
}

impl StoredPolicy {
    pub(crate) fn describe(&self) -> String {
        let policy = match self.policy {
            Policy::Notify => "notify (check daily, update when you confirm)",
            Policy::AutomaticWhenIdle => "automatic when idle (control, then idle workers)",
            Policy::Off => "off (no automatic checks)",
        };
        format!(
            "Update policy: {policy} · channel: {} · window: {}",
            channel_name(self.channel),
            self.window
        )
    }
}

/// Read the stored policy, defaulting to Notify on stable.
pub(crate) fn read_policy(data_dir: &Path) -> StoredPolicy {
    let file: Option<PolicyFile> = fs::read_to_string(data_dir.join(POLICY_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    match file {
        Some(file) => StoredPolicy {
            policy: file.policy,
            channel: if file.channel == "beta" {
                ReleaseChannel::Beta
            } else {
                ReleaseChannel::Stable
            },
            window: file.window,
        },
        None => StoredPolicy {
            policy: Policy::Notify,
            channel: ReleaseChannel::Stable,
            window: DEFAULT_WINDOW.into(),
        },
    }
}

/// Apply `policy`/`channel` changes (if any) and store the result.
pub(crate) fn update_policy(
    data_dir: &Path,
    mut stored: StoredPolicy,
    policy: Option<ReleasePolicyArg>,
    channel: Option<ReleaseChannel>,
) -> Result<StoredPolicy, ReleaseError> {
    if policy.is_none() && channel.is_none() {
        return Ok(stored);
    }
    if let Some(policy) = policy {
        stored.policy = match policy {
            ReleasePolicyArg::Notify => Policy::Notify,
            ReleasePolicyArg::Auto => Policy::AutomaticWhenIdle,
            ReleasePolicyArg::Off => Policy::Off,
        };
    }
    if let Some(channel) = channel {
        stored.channel = channel;
    }
    let file = PolicyFile {
        policy: stored.policy,
        channel: channel_name(stored.channel).into(),
        window: stored.window.clone(),
    };
    write_private(&data_dir.join(POLICY_FILE), &file)?;
    Ok(stored)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WatchFile {
    checked_at: Option<i64>,
    current: String,
    channel: String,
    available: Option<Available>,
    unsigned_found: Option<String>,
    last_result: Option<UpdateResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Available {
    version: String,
    signed: bool,
    published_at: String,
    notes_url: String,
}

/// The outcome of the last update or rollback.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct UpdateResult {
    pub(crate) state: String,
    pub(crate) version: String,
    pub(crate) previous: String,
    pub(crate) reason: Option<String>,
    pub(crate) at: i64,
}

fn read_watch(data_dir: &Path) -> WatchFile {
    fs::read_to_string(data_dir.join(WATCH_FILE))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Store one check result, keeping the last update result.
pub(crate) fn record_check(
    context: &Context,
    channel: ReleaseChannel,
    outcome: &CheckOutcome,
) -> Result<(), ReleaseError> {
    let mut watch = read_watch(&context.data_dir);
    watch.checked_at = Some(now_ms());
    watch.current = context.current.to_string();
    watch.channel = channel_name(channel).into();
    watch.available = outcome.available.as_ref().map(|candidate| Available {
        version: candidate.version.to_string(),
        signed: true,
        published_at: candidate.published_at.clone(),
        notes_url: candidate.notes_url.clone(),
    });
    watch.unsigned_found = outcome.unsigned_found.map(|version| version.to_string());
    write_private(&context.data_dir.join(WATCH_FILE), &watch)
}

/// Store the outcome of an update or rollback.
pub(crate) fn record_result(
    context: &Context,
    state: &str,
    version: Version,
    reason: Option<String>,
) -> Result<(), ReleaseError> {
    let mut watch = read_watch(&context.data_dir);
    watch.last_result = Some(UpdateResult {
        state: state.into(),
        version: version.to_string(),
        previous: context.current.to_string(),
        reason,
        at: now_ms(),
    });
    if state == "UPDATED" {
        watch.current = version.to_string();
        watch.available = None;
    }
    write_private(&context.data_dir.join(WATCH_FILE), &watch)
}

fn write_private<T: Serialize>(path: &Path, value: &T) -> Result<(), ReleaseError> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| ReleaseError::Refused(error.to_string()))?;
    let temporary = PathBuf::from(format!("{}.tmp.{}", path.display(), std::process::id()));
    let file_error = |action, source| ReleaseError::File {
        action,
        path: path.to_path_buf(),
        source,
    };
    fs::write(&temporary, &bytes).map_err(|source| file_error("write", source))?;
    set_mode(&temporary, 0o600);
    fs::rename(&temporary, path).map_err(|source| file_error("replace", source))
}
