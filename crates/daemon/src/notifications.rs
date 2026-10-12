//! Event-driven ntfy delivery with persistent retry state and safe presentation.

mod config;
mod delivery;
mod presentation;
mod runtime;

use config::ConfigSnapshot;
use presentation::Presentation;
use protocol::{
    CaptureLevel, Event, EventKind, MachineId, NotificationConfigurationState,
    NotificationDeliveryState, NotificationFacts, NotificationIntent, NotificationJournalEntry,
    NotificationRuleSet, NotificationRules, NotificationStatus, SessionId, SessionNotificationRule,
    SessionOrigin, Timestamp,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, RwLock},
};
use store::{NewNotificationDelivery, NotificationDelivery, Store};

const DEDUP_MS: i64 = 5 * 60 * 1_000;
const MAX_PENDING: u32 = 256;
const JOURNAL_LIMIT: u32 = 1_000;
const JOURNAL_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;
pub(super) const DELIVERY_BATCH: u32 = 32;
pub(super) const DELIVERY_CONCURRENCY: usize = 4;
pub(super) const RETRY_DELAYS_MS: [i64; 3] = [5_000, 30_000, 120_000];

/// Injectable notification clock.
pub type NotificationClock = Arc<dyn Fn() -> Timestamp + Send + Sync>;
/// Trusted current control origin, or `None` when this Machine is not linked.
pub type ControlOrigin = Arc<dyn Fn() -> Option<String> + Send + Sync>;
/// Optional durable native-Push ingress selected dynamically by provider state.
pub type NativeNotificationSink = Arc<
    dyn Fn(&NotificationIntent, Timestamp) -> Result<Option<NativeNotificationState>, String>
        + Send
        + Sync,
>;

/// Durable point reached by a native notification intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeNotificationState {
    /// Worker outbox is waiting for control acknowledgement.
    ForwardQueued,
    /// Control accepted the intent and created provider work.
    ControlAccepted,
    /// This control explicitly disabled external notification delivery.
    Disabled,
}

pub(super) type SendFuture = Pin<Box<dyn Future<Output = SendResult> + Send>>;
pub(super) type NotificationSender = Arc<dyn Fn(OutboundRequest) -> SendFuture + Send + Sync>;

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredRules {
    #[serde(default)]
    default: NotificationRuleSet,
    #[serde(default)]
    sessions: HashMap<SessionId, NotificationRuleSet>,
}

#[derive(Debug, Clone)]
pub(super) struct OutboundRequest {
    url: String,
    token: Option<String>,
    presentation: Presentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SendResult {
    Accepted,
    Timeout,
    ConnectionFailed,
    RateLimited(Option<u64>),
    ClientError,
    ServerError,
}

/// Local notification rules, outbox, and delivery client for one Machine.
#[derive(Clone)]
pub struct NotificationService {
    machine_id: MachineId,
    machine_name: String,
    config_path: PathBuf,
    owner_uid: u32,
    development: bool,
    config: Arc<RwLock<ConfigSnapshot>>,
    rules_path: PathBuf,
    rules: Arc<RwLock<StoredRules>>,
    store: Arc<Store>,
    clock: NotificationClock,
    control_origin: ControlOrigin,
    sender: NotificationSender,
    native_sink: Option<NativeNotificationSink>,
    delivery_lock: Arc<tokio::sync::Mutex<()>>,
}

impl NotificationService {
    /// Load configuration and rules, reporting redacted diagnostics.
    #[allow(clippy::too_many_arguments)]
    pub fn load(
        config_path: &Path,
        rules_path: PathBuf,
        machine_id: MachineId,
        machine_name: String,
        owner_uid: u32,
        development: bool,
        store: Arc<Store>,
        control_origin: ControlOrigin,
        clock: NotificationClock,
    ) -> (Self, Vec<String>) {
        let snapshot = config::load(config_path, owner_uid, development, clock());
        let mut diagnostics = snapshot
            .diagnostic
            .as_ref()
            .map(|value| vec![format!("{}: {value}", config_path.display())])
            .unwrap_or_default();
        let rules = match fs::read_to_string(&rules_path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|error| {
                diagnostics.push(format!("{}: {error}", rules_path.display()));
                StoredRules::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => StoredRules::default(),
            Err(error) => {
                diagnostics.push(format!("{}: {error}", rules_path.display()));
                StoredRules::default()
            }
        };
        let (sender, client_error) = delivery::reqwest_sender();
        if let Some(error) = client_error {
            diagnostics.push(error);
        }
        (
            Self {
                machine_id,
                machine_name,
                config_path: config_path.to_path_buf(),
                owner_uid,
                development,
                config: Arc::new(RwLock::new(snapshot)),
                rules_path,
                rules: Arc::new(RwLock::new(rules)),
                store,
                clock,
                control_origin,
                sender,
                native_sink: None,
                delivery_lock: Arc::new(tokio::sync::Mutex::new(())),
            },
            diagnostics,
        )
    }

    /// Attach native-Push ingress without changing the legacy ntfy loader.
    pub fn with_native_sink(mut self, sink: NativeNotificationSink) -> Self {
        self.native_sink = Some(sink);
        self
    }

    #[cfg(test)]
    fn with_sender(mut self, sender: NotificationSender) -> Self {
        self.sender = sender;
        self
    }

    /// Reload `notify.toml` atomically and return the secret-free status.
    pub fn reload_config(&self) -> NotificationStatus {
        let mut snapshot = config::load(
            &self.config_path,
            self.owner_uid,
            self.development,
            (self.clock)(),
        );
        if let Ok(mut current) = self.config.write() {
            if snapshot.reloaded_at.is_none() {
                snapshot.reloaded_at = current.reloaded_at;
            }
            *current = snapshot;
        }
        self.status()
    }

    /// Secret-free configuration shown in Settings.
    pub fn status(&self) -> NotificationStatus {
        let Ok(snapshot) = self.config.read() else {
            return unavailable_status("notification configuration lock is unavailable");
        };
        NotificationStatus {
            configured: snapshot.config.is_some(),
            server: snapshot.config.as_ref().map(|value| value.server.clone()),
            topic: snapshot.config.as_ref().map(|value| value.topic.clone()),
            token_configured: snapshot
                .config
                .as_ref()
                .and_then(|value| value.token.as_ref())
                .is_some(),
            configuration_state: Some(snapshot.state),
            diagnostic: snapshot.diagnostic.clone(),
            reloaded_at: snapshot.reloaded_at,
        }
    }

    /// Snapshot current rules.
    pub fn rules(&self) -> NotificationRules {
        let Ok(rules) = self.rules.read() else {
            return NotificationRules {
                default: NotificationRuleSet::default(),
                sessions: Vec::new(),
            };
        };
        let mut sessions: Vec<_> = rules
            .sessions
            .iter()
            .map(|(session_id, rules)| SessionNotificationRule {
                session_id: *session_id,
                rules: *rules,
            })
            .collect();
        sessions.sort_by_key(|rule| rule.session_id);
        NotificationRules {
            default: rules.default,
            sessions,
        }
    }

    /// Replace the default rule or one Session override and persist it privately.
    pub fn set_rule(
        &self,
        session_id: Option<SessionId>,
        value: NotificationRuleSet,
    ) -> Result<(), String> {
        let text = {
            let mut rules = self
                .rules
                .write()
                .map_err(|_| "notification rules lock poisoned".to_string())?;
            match session_id {
                Some(id) => {
                    rules.sessions.insert(id, value);
                }
                None => rules.default = value,
            }
            serde_json::to_vec_pretty(&*rules).map_err(|error| error.to_string())?
        };
        if let Some(parent) = self.rules_path.parent() {
            crate::private_fs::ensure_private_dir(parent).map_err(|error| error.to_string())?;
        }
        let mut options = crate::private_fs::private_file_options();
        options.write(true).create(true).truncate(true);
        use std::io::Write;
        options
            .open(&self.rules_path)
            .and_then(|mut file| file.write_all(&text).and_then(|()| file.sync_all()))
            .map_err(|error| {
                format!(
                    "failed to write notification rules '{}': {error}",
                    self.rules_path.display()
                )
            })
    }

    /// Record a decision for one structured Event without performing network I/O.
    pub fn enqueue(&self, event: &Event, capture: CaptureLevel) -> Result<bool, String> {
        if !capture.supports_evidence(&event.kind) {
            return Ok(false);
        }
        let Some(category) = presentation::category(&event.kind) else {
            return Ok(false);
        };
        let session_row = self
            .store
            .session(event.session_id)
            .map_err(|error| error.to_string())?;
        if let EventKind::CommandFinished { duration_ms, .. } = &event.kind {
            if !finish_notifies(
                session_row.as_ref().map(|session| session.origin),
                duration_ms.0,
            ) {
                return Ok(false);
            }
        }
        let enabled = self
            .rules
            .read()
            .map_err(|_| "notification rules lock poisoned".to_string())
            .map(|rules| {
                let rules = rules
                    .sessions
                    .get(&event.session_id)
                    .copied()
                    .unwrap_or(rules.default);
                category_enabled(rules, &event.kind)
            })?;
        let ntfy_configured = self
            .config
            .read()
            .map(|value| value.state == NotificationConfigurationState::Configured)
            .unwrap_or(false);
        let session_label = session_row
            .and_then(|session| session.title)
            .and_then(safe_session_label);
        let intent = notification_intent(&self.machine_id, event, session_label)?;
        let native_state = if enabled {
            self.native_sink
                .as_ref()
                .map(|sink| sink(&intent, (self.clock)()))
                .transpose()?
                .flatten()
        } else {
            None
        };
        let state = if !enabled {
            NotificationDeliveryState::RuleDisabled
        } else if let Some(native) = native_state {
            match native {
                NativeNotificationState::ForwardQueued => NotificationDeliveryState::ForwardQueued,
                NativeNotificationState::ControlAccepted => {
                    NotificationDeliveryState::ControlAccepted
                }
                NativeNotificationState::Disabled => NotificationDeliveryState::NotConfigured,
            }
        } else if !ntfy_configured {
            NotificationDeliveryState::NotConfigured
        } else {
            NotificationDeliveryState::Queued
        };
        let now = (self.clock)();
        self.store
            .prune_notification_deliveries(Timestamp(now.0.saturating_sub(JOURNAL_RETENTION_MS)))
            .map_err(|error| error.to_string())?;
        let deduplicate_since = (matches!(
            state,
            NotificationDeliveryState::Queued
                | NotificationDeliveryState::ForwardQueued
                | NotificationDeliveryState::ControlAccepted
        ) && !matches!(event.kind, EventKind::CommandFinished { .. }))
        .then_some(Timestamp(now.0.saturating_sub(DEDUP_MS)));
        let record = self
            .store
            .record_notification_delivery(&NewNotificationDelivery {
                event_id: event.id,
                machine_id: self.machine_id.clone(),
                session_id: event.session_id,
                category,
                state,
                at: now,
                deduplicate_since,
                pending_limit: MAX_PENDING,
            })
            .map_err(|error| error.to_string())?;
        Ok(matches!(
            record.state,
            NotificationDeliveryState::Queued
                | NotificationDeliveryState::ForwardQueued
                | NotificationDeliveryState::ControlAccepted
        ))
    }

    /// Return newest journal entries first, rendered from structured Events.
    pub fn journal(&self) -> Vec<NotificationJournalEntry> {
        self.store
            .notification_deliveries(JOURNAL_LIMIT)
            .map(|records| {
                records
                    .into_iter()
                    .map(|record| self.journal_entry(record))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn journal_entry(&self, record: NotificationDelivery) -> NotificationJournalEntry {
        let text = self
            .store
            .notification_event(record.event_id)
            .ok()
            .flatten()
            .and_then(|event| {
                let title = self
                    .store
                    .session(record.session_id)
                    .ok()
                    .flatten()
                    .and_then(|session| session.title);
                presentation::render(
                    &event.kind,
                    &self.machine_name,
                    title.as_deref(),
                    &self.machine_id,
                    record.session_id,
                    (self.control_origin)().as_deref(),
                )
                .map(|value| format!("{}: {}", value.title, value.body))
            })
            .unwrap_or_else(|| format!("Sentinal: Session {} notification", record.session_id.0));
        NotificationJournalEntry {
            id: record.id,
            machine_id: record.machine_id,
            session_id: record.session_id,
            text,
            at: record.updated_at,
            delivered: record.state == NotificationDeliveryState::Delivered,
            event_id: Some(record.event_id),
            category: Some(record.category),
            state: Some(record.state),
            attempts: Some(record.attempts),
            result_class: record.result_class,
            request_bytes: record.request_bytes,
        }
    }
}

fn notification_intent(
    machine_id: &MachineId,
    event: &Event,
    session_label: Option<String>,
) -> Result<NotificationIntent, String> {
    let facts = match &event.kind {
        EventKind::CommandFinished { exit_code, .. } => NotificationFacts::CommandFinished {
            exit_code: *exit_code,
        },
        EventKind::ErrorDetected { .. } => NotificationFacts::Error,
        EventKind::WaitingForInput { .. } => NotificationFacts::Waiting,
        EventKind::PossiblyStalled { silent_for_ms } => NotificationFacts::Stalled {
            silent_for_ms: silent_for_ms.0,
        },
        EventKind::ResourceHigh { resource, percent } => NotificationFacts::ResourceHigh {
            resource: *resource,
            percent: *percent,
        },
        EventKind::CheckpointCreated { .. } => NotificationFacts::Checkpoint,
        EventKind::ProgressUpdated {
            current,
            total,
            unit,
        } => NotificationFacts::Progress {
            current: *current,
            total: *total,
            unit: unit.clone(),
        },
        _ => return Err("Event has no native notification facts".into()),
    };
    let identity = format!("{}:{}:{}", machine_id.0, event.id.0, event.session_id.0);
    let intent = NotificationIntent {
        intent_id: format!("ni_{}", crate::link_crypto::sha256_hex(&identity)),
        event_id: event.id,
        session_id: event.session_id,
        occurred_at: event.at,
        session_label,
        facts,
    };
    intent.validate().map_err(|error| error.to_string())?;
    Ok(intent)
}

fn safe_session_label(value: String) -> Option<String> {
    let candidate = NotificationIntent {
        intent_id: format!("ni_{}", "0".repeat(64)),
        event_id: protocol::EventId(1),
        session_id: SessionId(1),
        occurred_at: Timestamp(0),
        session_label: Some(value.clone()),
        facts: NotificationFacts::Waiting,
    };
    candidate.validate().is_ok().then_some(value)
}

fn unavailable_status(message: &str) -> NotificationStatus {
    NotificationStatus {
        configured: false,
        server: None,
        topic: None,
        token_configured: false,
        configuration_state: Some(NotificationConfigurationState::Invalid),
        diagnostic: Some(message.into()),
        reloaded_at: None,
    }
}

fn category_enabled(rules: NotificationRuleSet, kind: &EventKind) -> bool {
    match kind {
        EventKind::CommandFinished { exit_code: 0, .. } => rules.finished,
        EventKind::CommandFinished { .. } | EventKind::ErrorDetected { .. } => rules.error,
        EventKind::ResourceHigh { .. } => rules.error,
        EventKind::PossiblyStalled { .. } => rules.stalled,
        EventKind::WaitingForInput { .. } => rules.waiting,
        EventKind::CheckpointCreated { .. } => rules.checkpoint,
        EventKind::ProgressUpdated { .. } => rules.progress,
        _ => false,
    }
}

pub use runtime::{spawn_notification_workers, spawn_notifier};

/// A finished Command is notification-worthy for a `sentinal watch` Session
/// (its one defining Command is the job) or, in any other Session (a shell,
/// hooked terminal, or auto-wrapped shell running many Commands), only when it
/// ran at least `LONG_COMMAND_MS`, so typing `ls` never notifies but a long
/// build still does. An unknown Session keeps notifying.
pub(crate) fn finish_notifies(origin: Option<SessionOrigin>, duration_ms: u64) -> bool {
    match origin {
        Some(SessionOrigin::Watch) | None => true,
        Some(_) => duration_ms >= LONG_COMMAND_MS,
    }
}

/// Minimum run time for a non-Watch Command to notify when it finishes.
pub(crate) const LONG_COMMAND_MS: u64 = 60_000;

#[cfg(test)]
mod tests;
