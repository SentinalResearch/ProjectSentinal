//! Notification rules and delivery journal entries.

use crate::{MachineId, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// Event categories a user may enable or disable for one Session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct NotificationRuleSet {
    /// Notify when a command finishes successfully.
    pub finished: bool,
    /// Notify when a command or watcher reports an error.
    pub error: bool,
    /// Notify when a Session may be stalled.
    pub stalled: bool,
    /// Notify when a Session waits for input.
    pub waiting: bool,
    /// Notify when a checkpoint is created.
    pub checkpoint: bool,
    /// Notify on progress updates.
    pub progress: bool,
}

impl Default for NotificationRuleSet {
    fn default() -> Self {
        Self {
            finished: true,
            error: true,
            stalled: true,
            waiting: true,
            checkpoint: false,
            progress: false,
        }
    }
}

/// Rules configured on one daemon, including its default rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct NotificationRules {
    /// Rule inherited by Sessions without an override.
    pub default: NotificationRuleSet,
    /// Per-Session overrides.
    pub sessions: Vec<SessionNotificationRule>,
}

/// One per-Session notification override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct SessionNotificationRule {
    /// Daemon-local Session ID.
    pub session_id: SessionId,
    /// Complete override for the Session.
    pub rules: NotificationRuleSet,
}

/// Safe notification configuration exposed to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct NotificationStatus {
    /// Whether a valid `notify.toml` was loaded.
    pub configured: bool,
    /// Configured ntfy server without credentials.
    pub server: Option<String>,
    /// Configured ntfy topic.
    pub topic: Option<String>,
    /// Whether an authentication token is configured.
    pub token_configured: bool,
}

/// A bounded record of one attempted notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct NotificationJournalEntry {
    /// Stable local journal ID.
    pub id: u64,
    /// Machine that emitted the notification.
    pub machine_id: MachineId,
    /// Session that caused it.
    pub session_id: SessionId,
    /// Secret-free notification text.
    pub text: String,
    /// Delivery timestamp.
    pub at: Timestamp,
    /// Whether ntfy accepted the request.
    pub delivered: bool,
}
