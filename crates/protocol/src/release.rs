//! The control's owner-signed release state and update requests (4.E §11b),
//! served at `/api/v1/control/release` to the owner's dashboard only.

use crate::{ProtocolError, Timestamp};
use serde::{Deserialize, Serialize};

/// Which GitHub releases the control follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReleaseChannel {
    /// Published releases only.
    Stable,
    /// Published releases and pre-releases.
    Beta,
}

/// How the control handles new signed releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReleasePolicy {
    /// Check daily, notify, and update only when the owner confirms.
    Notify,
    /// Update the control automatically in the window when it is idle.
    AutomaticWhenIdle,
    /// Never contact GitHub automatically.
    Off,
}

/// A newer release whose `SHA256SUMS` verified against the pinned keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct AvailableRelease {
    /// Strict `MAJOR.MINOR.PATCH`.
    pub version: String,
    /// Publication time as reported by GitHub (RFC 3339).
    pub published_at: String,
    /// Release notes page.
    pub notes_url: String,
}

/// Lifecycle of one control update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ControlUpdateState {
    /// The detached updater is working; the dashboard may disconnect.
    Running,
    /// The control runs the new release and passed its health check.
    Updated,
    /// The new release failed its health check; the previous one is restored.
    RolledBack,
    /// The update failed before replacing anything.
    Failed,
    /// No result arrived in time; check the control.
    Unknown,
}

/// The most recent or in-progress control update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ControlUpdateStatus {
    /// Stable update identifier.
    pub update_id: String,
    /// Target version.
    pub version: String,
    /// Version before the update.
    pub previous: String,
    /// Current state.
    pub state: ControlUpdateState,
    /// Owner-readable reason for a rollback or failure.
    pub reason: Option<String>,
    /// When the update started.
    pub started_at: Option<Timestamp>,
    /// When a result was recorded.
    pub finished_at: Option<Timestamp>,
}

/// Everything the dashboard shows about this control's releases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ControlReleaseView {
    /// Installed release.
    pub current: String,
    /// Followed channel.
    pub channel: ReleaseChannel,
    /// Update policy.
    pub policy: ReleasePolicy,
    /// Local maintenance window, `HH:MM-HH:MM`.
    pub window: String,
    /// Newer signed release, if any.
    pub available: Option<AvailableRelease>,
    /// Newest newer release that was not signed by a trusted key.
    pub unsigned_found: Option<String>,
    /// When the last check finished.
    pub last_checked_at: Option<Timestamp>,
    /// Latest or running update.
    pub update: Option<ControlUpdateStatus>,
    /// Live FULL Sessions an update would interrupt (until Reliability R1).
    pub live_full_sessions: u32,
}

/// Owner request to update the control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ControlUpdateRequest {
    /// Exact version, which must be the offered signed release.
    pub version: String,
    /// The owner confirmed interrupting live FULL Sessions.
    pub allow_interrupt: bool,
}

impl ControlUpdateRequest {
    /// Accept only a strict `MAJOR.MINOR.PATCH` version.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if strict_version(&self.version) {
            Ok(())
        } else {
            Err(ProtocolError::InvalidReleaseVersion(
                self.version.escape_debug().to_string(),
            ))
        }
    }
}

/// Accepted update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ControlUpdateStarted {
    /// Identifier to follow in `ControlReleaseView.update`.
    pub update_id: String,
}

/// Owner request to change the update policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ReleasePolicyRequest {
    /// New policy.
    pub policy: ReleasePolicy,
    /// New channel.
    pub channel: ReleaseChannel,
}

/// True for `MAJOR.MINOR.PATCH` with no leading zeros and ≤ 9 digits each.
pub fn strict_version(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.len() <= 9
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        })
}
