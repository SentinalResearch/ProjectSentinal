//! Notification rules and delivery journal entries.

use crate::{EventId, MachineId, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// Whether one Machine can currently deliver notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationConfigurationState {
    /// No `notify.toml` is present on this Machine.
    Unavailable,
    /// A valid configuration is loaded.
    Configured,
    /// A configuration exists but failed validation.
    Invalid,
}

/// Stable notification category derived from a structured Event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationCategory {
    /// A Command completed successfully.
    Completed,
    /// A Command failed.
    Failed,
    /// A watcher detected an error.
    Error,
    /// Work is waiting for input.
    Waiting,
    /// Work may be stalled.
    Stalled,
    /// A resource crossed its notification threshold.
    ResourceHigh,
    /// A checkpoint was created.
    Checkpoint,
    /// Work reported progress.
    Progress,
}

/// Current decision or delivery state for one Event notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationDeliveryState {
    /// The effective rule disabled this category.
    RuleDisabled,
    /// A recent notification already represented this category.
    Deduplicated,
    /// This Machine has no usable notification configuration.
    NotConfigured,
    /// Delivery is waiting for a worker.
    Queued,
    /// A transient failure will be retried.
    Retrying,
    /// The ntfy server accepted the request.
    Delivered,
    /// Delivery stopped after a permanent or exhausted failure.
    PermanentlyFailed,
}

/// Bounded result classification that never contains an upstream body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationResultClass {
    /// The ntfy server accepted the request.
    Accepted,
    /// The request timed out.
    Timeout,
    /// DNS or transport connection failed.
    ConnectionFailed,
    /// The server rate-limited delivery.
    RateLimited,
    /// A non-retryable client response was returned.
    ClientError,
    /// A retryable server response was returned.
    ServerError,
    /// The bounded delivery queue was full.
    QueueFull,
    /// Daemon shutdown returned the attempt to the queue.
    Shutdown,
}

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
    /// Detailed configuration state; absent when talking to a pre-4.C daemon.
    #[serde(default)]
    pub configuration_state: Option<NotificationConfigurationState>,
    /// Redacted validation detail suitable for an owner UI.
    #[serde(default)]
    pub diagnostic: Option<String>,
    /// Last successful configuration load time.
    #[serde(default)]
    pub reloaded_at: Option<Timestamp>,
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
    /// Stored Event that caused the decision.
    #[serde(default)]
    pub event_id: Option<EventId>,
    /// Stable category derived from the Event.
    #[serde(default)]
    pub category: Option<NotificationCategory>,
    /// Current decision or delivery state.
    #[serde(default)]
    pub state: Option<NotificationDeliveryState>,
    /// Number of external delivery attempts made.
    #[serde(default)]
    pub attempts: Option<u32>,
    /// Bounded result class, without an upstream response body.
    #[serde(default)]
    pub result_class: Option<NotificationResultClass>,
    /// Bytes in the most recently rendered safe request.
    #[serde(default)]
    pub request_bytes: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_status_and_journal_receive_safe_delivery_defaults() {
        let status: NotificationStatus = serde_json::from_value(serde_json::json!({
            "configured": true,
            "server": "https://ntfy.example",
            "topic": "owner-topic",
            "token_configured": true
        }))
        .unwrap();
        assert_eq!(status.configuration_state, None);
        assert_eq!(status.diagnostic, None);
        assert_eq!(status.reloaded_at, None);

        let entry: NotificationJournalEntry = serde_json::from_value(serde_json::json!({
            "id": 1,
            "machine_id": "machine-a",
            "session_id": 7,
            "text": "safe preview",
            "at": 1000,
            "delivered": false
        }))
        .unwrap();
        assert_eq!(entry.event_id, None);
        assert_eq!(entry.category, None);
        assert_eq!(entry.state, None);
        assert_eq!(entry.attempts, None);
        assert_eq!(entry.result_class, None);
        assert_eq!(entry.request_bytes, None);
    }

    #[test]
    fn detailed_journal_round_trips_all_delivery_metadata() {
        let entry = NotificationJournalEntry {
            id: 42,
            machine_id: MachineId("machine-a".into()),
            session_id: SessionId(7),
            text: "Machine A · Session 7: waiting for input".into(),
            at: Timestamp(2_000),
            delivered: false,
            event_id: Some(crate::EventId(99)),
            category: Some(NotificationCategory::Waiting),
            state: Some(NotificationDeliveryState::Retrying),
            attempts: Some(2),
            result_class: Some(NotificationResultClass::Timeout),
            request_bytes: Some(187),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let decoded: NotificationJournalEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, entry);
    }
}
