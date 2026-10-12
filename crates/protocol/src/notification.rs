//! Notification rules and delivery journal entries.

use crate::{EventId, MachineId, ProtocolError, Resource, SessionId, Timestamp};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Maximum encoded native-notification intent identifier length.
pub const MAX_NOTIFICATION_INTENT_ID_BYTES: usize = 67;
/// Maximum safe Session label copied into a notification intent.
pub const MAX_NOTIFICATION_LABEL_BYTES: usize = 192;
/// Maximum browser Push endpoint length.
pub const MAX_PUSH_ENDPOINT_BYTES: usize = 2_048;
/// Maximum owner-visible device label length.
pub const MAX_PUSH_DEVICE_LABEL_BYTES: usize = 192;
/// Maximum intents accepted in one worker upload.
pub const MAX_NOTIFICATION_INTENTS_PER_BATCH: usize = 32;

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
    /// The control's own release (available, updated, or rolled back).
    Release,
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
    /// A worker intent is durably waiting for control acknowledgement.
    ForwardQueued,
    /// The control durably accepted the intent for provider fan-out.
    ControlAccepted,
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
    /// The control durably accepted a worker intent.
    ControlAccepted,
    /// The browser provider declared this subscription expired.
    SubscriptionExpired,
    /// The provider refused the bounded encrypted payload size.
    PayloadTooLarge,
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
    /// Whether the selected external provider accepted the request.
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

/// Safe structured facts from which the control renders one notification.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationFacts {
    /// A Command finished with a known exit code.
    CommandFinished {
        /// Exact shell exit code.
        exit_code: i32,
    },
    /// A deterministic watcher detected an error without copying its line.
    Error,
    /// Work is waiting for owner input without copying the prompt.
    Waiting,
    /// Work has been inactive for a deterministic interval.
    Stalled {
        /// Inactivity duration in milliseconds.
        silent_for_ms: u64,
    },
    /// A resource crossed its deterministic threshold.
    ResourceHigh {
        /// Resource that crossed the threshold.
        resource: Resource,
        /// Bounded percentage observed.
        percent: f32,
    },
    /// A checkpoint was created without copying its path.
    Checkpoint,
    /// Bounded progress counters.
    Progress {
        /// Current progress counter.
        current: u64,
        /// Optional total counter.
        total: Option<u64>,
        /// Bounded safe unit such as `epoch`.
        unit: String,
    },
}

impl NotificationFacts {
    /// Derive the stable delivery category; callers cannot claim a mismatch.
    pub const fn category(&self) -> NotificationCategory {
        match self {
            NotificationFacts::CommandFinished { exit_code: 0 } => NotificationCategory::Completed,
            NotificationFacts::CommandFinished { .. } => NotificationCategory::Failed,
            NotificationFacts::Error => NotificationCategory::Error,
            NotificationFacts::Waiting => NotificationCategory::Waiting,
            NotificationFacts::Stalled { .. } => NotificationCategory::Stalled,
            NotificationFacts::ResourceHigh { .. } => NotificationCategory::ResourceHigh,
            NotificationFacts::Checkpoint => NotificationCategory::Checkpoint,
            NotificationFacts::Progress { .. } => NotificationCategory::Progress,
        }
    }

    /// Validate all bounded numeric and textual fact values.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        match self {
            NotificationFacts::Stalled { silent_for_ms } if *silent_for_ms == 0 => {
                invalid("facts.silent_for_ms", "must be greater than zero")
            }
            NotificationFacts::ResourceHigh { percent, .. }
                if !percent.is_finite() || !(0.0..=100.0).contains(percent) =>
            {
                invalid("facts.percent", "must be finite and between 0 and 100")
            }
            NotificationFacts::Progress {
                current,
                total: Some(total),
                ..
            } if *total == 0 || current > total => invalid(
                "facts.progress",
                "requires a non-zero total greater than or equal to current",
            ),
            NotificationFacts::Progress { unit, .. } => {
                validate_safe_text("facts.unit", unit, 32, false)
            }
            _ => Ok(()),
        }
    }
}

/// One privacy-safe Event-derived intent uploaded by a worker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct NotificationIntent {
    /// Deterministic idempotency identifier.
    pub intent_id: String,
    /// Worker-local stored Event identifier.
    pub event_id: EventId,
    /// Worker-local Session identifier.
    pub session_id: SessionId,
    /// Event timestamp in Unix milliseconds.
    pub occurred_at: Timestamp,
    /// Optional bounded Session label; never a Command fallback.
    pub session_label: Option<String>,
    /// Closed safe facts used for control-side presentation.
    pub facts: NotificationFacts,
}

impl NotificationIntent {
    /// Validate identity, bounds, labels, and structured facts.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.event_id.0 == 0 {
            return Err(ProtocolError::InvalidIdentifier { field: "event_id" });
        }
        if self.session_id.0 == 0 {
            return Err(ProtocolError::InvalidIdentifier {
                field: "session_id",
            });
        }
        if !valid_prefixed_hex(&self.intent_id, "ni_", 64) {
            return invalid(
                "intent_id",
                "must be 'ni_' followed by 64 lowercase hexadecimal characters",
            );
        }
        if let Some(label) = self.session_label.as_deref() {
            validate_safe_text("session_label", label, MAX_NOTIFICATION_LABEL_BYTES, false)?;
            if label.chars().count() > 48 {
                return invalid("session_label", "must contain at most 48 characters");
            }
        }
        self.facts.validate()
    }
}

/// Bounded worker-to-control upload of native-notification intents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct NotificationIntentBatch {
    /// Claimed Machine; the control binds this to the bearer credential.
    pub machine_id: MachineId,
    /// Worker send timestamp.
    pub sent_at: Timestamp,
    /// At most 32 ordered intents.
    pub intents: Vec<NotificationIntent>,
}

impl NotificationIntentBatch {
    /// Validate batch cardinality and every intent.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.intents.is_empty() {
            return invalid("intents", "must contain at least one intent");
        }
        if self.intents.len() > MAX_NOTIFICATION_INTENTS_PER_BATCH {
            return invalid("intents", "must contain at most 32 intents");
        }
        self.intents
            .iter()
            .try_for_each(NotificationIntent::validate)
    }
}

/// Stable reason one item in an otherwise valid batch was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum NotificationIntentRejectionCode {
    /// Intent timestamp exceeded retention.
    TooOld,
    /// Intent timestamp was too far in the future.
    ClockSkew,
    /// Structured facts or bounds were invalid.
    InvalidFacts,
    /// Referenced Session is unknown to the worker/control view.
    UnknownSession,
}

/// Bounded per-item rejection without echoing input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct NotificationIntentRejection {
    /// Rejected idempotency identifier.
    pub intent_id: String,
    /// Stable reason code.
    pub code: NotificationIntentRejectionCode,
}

/// Durable control acceptance result for one worker batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct NotificationIntentAcknowledgement {
    /// Newly accepted identifiers.
    pub accepted: Vec<String>,
    /// Previously accepted identifiers.
    pub duplicates: Vec<String>,
    /// Bounded rejected items.
    pub rejected: Vec<NotificationIntentRejection>,
}

/// Mutually exclusive owner-selected external notification provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PushProviderMode {
    /// Store decisions without attempting external delivery.
    Disabled,
    /// Control-owned encrypted Web Push.
    WebPush,
    /// Existing per-Machine ntfy delivery compatibility path.
    NtfyLegacy,
}

/// Owner mutation selecting exactly one external notification provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PushProviderModeRequest {
    /// Desired mutually exclusive provider.
    pub mode: PushProviderMode,
}

/// Public VAPID material exposed only to an authenticated owner browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PushPublicKey {
    /// Stable non-secret key identifier.
    pub key_id: String,
    /// Uncompressed P-256 public key encoded as base64url without padding.
    pub public_key: String,
    /// Validated control origin used as the VAPID subject.
    pub subject_origin: String,
}

/// Browser-managed subscription keys; Debug intentionally redacts them.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PushSubscriptionKeys {
    /// Browser ECDH public key as base64url without padding.
    pub p256dh: String,
    /// Browser authentication secret as base64url without padding.
    pub auth: String,
}

impl fmt::Debug for PushSubscriptionKeys {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PushSubscriptionKeys([redacted])")
    }
}

/// Owner-authenticated browser subscription mutation.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PushSubscriptionRequest {
    /// Owner-visible device label.
    pub device_label: String,
    /// Browser-managed opaque HTTPS endpoint.
    pub endpoint: String,
    /// Optional browser-reported expiry in Unix milliseconds.
    pub expiration_time: Option<Timestamp>,
    /// Browser-managed encryption material.
    pub keys: PushSubscriptionKeys,
}

impl fmt::Debug for PushSubscriptionRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PushSubscriptionRequest")
            .field("device_label", &self.device_label)
            .field("endpoint", &"[redacted]")
            .field("expiration_time", &self.expiration_time)
            .field("keys", &self.keys)
            .finish()
    }
}

impl PushSubscriptionRequest {
    /// Validate bounded textual and browser-key shapes.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        self.validate_shape()?;
        if !base64url_shape(&self.keys.p256dh, 87) {
            return invalid(
                "keys.p256dh",
                "must encode one 65-byte uncompressed P-256 key",
            );
        }
        if !base64url_shape(&self.keys.auth, 22) {
            return invalid("keys.auth", "must encode one 16-byte authentication secret");
        }
        Ok(())
    }

    /// Validate fields that do not require decoding browser key material.
    pub fn validate_shape(&self) -> Result<(), ProtocolError> {
        validate_safe_text(
            "device_label",
            &self.device_label,
            MAX_PUSH_DEVICE_LABEL_BYTES,
            false,
        )?;
        if self.device_label.chars().count() > 48 {
            return invalid("device_label", "must contain at most 48 characters");
        }
        if self.endpoint.len() > MAX_PUSH_ENDPOINT_BYTES {
            return Err(ProtocolError::FieldTooLong {
                field: "endpoint",
                actual: self.endpoint.len(),
                maximum: MAX_PUSH_ENDPOINT_BYTES,
            });
        }
        if !self.endpoint.starts_with("https://") {
            return invalid("endpoint", "must use HTTPS");
        }
        if self.endpoint.chars().any(char::is_control) {
            return invalid("endpoint", "must not contain control characters");
        }
        Ok(())
    }
}

/// Control-side lifecycle of one browser subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PushSubscriptionState {
    /// Delivery is permitted.
    Active,
    /// Browser push service reported expiration.
    Expired,
    /// Owner disabled this device.
    Disabled,
    /// A permanent provider error requires owner attention.
    Error,
}

/// Secret-free subscription metadata shown to the owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PushSubscriptionInfo {
    /// Stable non-secret subscription identifier.
    pub subscription_id: String,
    /// Owner-visible device label.
    pub device_label: String,
    /// Current lifecycle state.
    pub state: PushSubscriptionState,
    /// Creation timestamp.
    pub created_at: Timestamp,
    /// Last browser refresh or state change.
    pub updated_at: Timestamp,
    /// Last provider acceptance for this device.
    pub last_success_at: Option<Timestamp>,
    /// Optional browser-reported expiration.
    pub expiration_time: Option<Timestamp>,
}

/// Owner-visible provider state and devices without subscription secrets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct PushSubscriptionList {
    /// Mutually exclusive selected provider.
    pub provider_mode: PushProviderMode,
    /// Bounded owner device list.
    pub devices: Vec<PushSubscriptionInfo>,
}

fn invalid<T>(field: &'static str, reason: &'static str) -> Result<T, ProtocolError> {
    Err(ProtocolError::InvalidNotificationField { field, reason })
}

fn valid_prefixed_hex(value: &str, prefix: &str, digits: usize) -> bool {
    value.len() == prefix.len() + digits
        && value.starts_with(prefix)
        && value[prefix.len()..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn base64url_shape(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn validate_safe_text(
    field: &'static str,
    value: &str,
    maximum: usize,
    allow_empty: bool,
) -> Result<(), ProtocolError> {
    if value.is_empty() && !allow_empty {
        return Err(ProtocolError::EmptyField { field });
    }
    if value.len() > maximum {
        return Err(ProtocolError::FieldTooLong {
            field,
            actual: value.len(),
            maximum,
        });
    }
    if value.chars().any(|character| {
        character.is_control()
            || matches!(character, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
    }) {
        return invalid(
            field,
            "must not contain control or bidirectional override characters",
        );
    }
    Ok(())
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
