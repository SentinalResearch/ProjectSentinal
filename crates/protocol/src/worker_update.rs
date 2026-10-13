//! Updating one worker from the dashboard (4.E §11). The owner asks the
//! control; the control forwards only a validated version over the link; the
//! worker installs from its own pinned control origin and reports the result
//! in later heartbeats.

use crate::{release::strict_version, ProtocolError, Timestamp};
use serde::{Deserialize, Serialize};

/// Owner → control (`POST /api/v1/machines/{id}/update`), and control →
/// worker (`POST /api/v1/link/update`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct WorkerUpdateRequest {
    /// Exact version on the control's stable channel.
    pub version: String,
    /// The owner confirmed interrupting live FULL Sessions.
    pub allow_interrupt: bool,
}

impl WorkerUpdateRequest {
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

/// Lifecycle of one worker update.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum WorkerUpdateState {
    /// The worker accepted and started its updater.
    Started,
    /// The worker runs a new release.
    Installed,
    /// The update failed; the previous release kept or restored.
    Failed,
    /// No report within five minutes.
    Unknown,
}

/// The latest update of one worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct WorkerUpdateStatus {
    /// Requested version.
    pub version: String,
    /// Version before the update.
    pub previous: String,
    /// Current state.
    pub state: WorkerUpdateState,
    /// Why it failed (bounded, no paths or secrets).
    pub reason: Option<String>,
    /// When the worker accepted it.
    pub started_at: Timestamp,
}
