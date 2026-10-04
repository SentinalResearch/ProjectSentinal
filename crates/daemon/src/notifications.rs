//! Event-driven ntfy delivery with per-Session rules and deterministic deduplication.

use protocol::{
    CaptureLevel, Event, EventKind, MachineId, NotificationJournalEntry, NotificationRuleSet,
    NotificationRules, NotificationStatus, SessionId, SessionNotificationRule, Timestamp,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, RwLock},
};

const DEDUP_MS: i64 = 5 * 60 * 1_000;
const MAX_JOURNAL: usize = 100;

/// Injectable notification clock.
pub type NotificationClock = Arc<dyn Fn() -> Timestamp + Send + Sync>;

#[derive(Debug, Clone, Deserialize)]
struct NotifyConfig {
    server: String,
    topic: String,
    token: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredRules {
    #[serde(default)]
    default: NotificationRuleSet,
    #[serde(default)]
    sessions: HashMap<SessionId, NotificationRuleSet>,
}

/// Local notification rules and delivery client for one Machine.
#[derive(Clone)]
pub struct NotificationService {
    machine_id: MachineId,
    config: Option<NotifyConfig>,
    rules_path: PathBuf,
    rules: Arc<RwLock<StoredRules>>,
    dedup: Arc<Mutex<HashMap<(SessionId, &'static str), Timestamp>>>,
    journal: Arc<Mutex<Vec<NotificationJournalEntry>>>,
    next_id: Arc<Mutex<u64>>,
    clock: NotificationClock,
    client: reqwest::Client,
}

impl NotificationService {
    /// Load optional ntfy configuration and persisted rules without exposing the token.
    pub fn load(
        config_path: &Path,
        rules_path: PathBuf,
        machine_id: MachineId,
        clock: NotificationClock,
    ) -> (Self, Vec<String>) {
        let mut diagnostics = Vec::new();
        let config = match fs::read_to_string(config_path) {
            Ok(text) => match toml::from_str::<NotifyConfig>(&text) {
                Ok(config) if valid_config(&config) => Some(config),
                Ok(_) => {
                    diagnostics.push(format!(
                        "{}: server must be HTTPS (plain HTTP only to loopback or Tailscale), topic must be a safe non-empty segment",
                        config_path.display()
                    ));
                    None
                }
                Err(error) => {
                    diagnostics.push(format!("{}: {error}", config_path.display()));
                    None
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                diagnostics.push(format!("{}: {error}", config_path.display()));
                None
            }
        };
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
        (
            Self {
                machine_id,
                config,
                rules_path,
                rules: Arc::new(RwLock::new(rules)),
                dedup: Arc::new(Mutex::new(HashMap::new())),
                journal: Arc::new(Mutex::new(Vec::new())),
                next_id: Arc::new(Mutex::new(1)),
                clock,
                client: reqwest::Client::new(),
            },
            diagnostics,
        )
    }

    /// Secret-free configuration shown in Settings.
    pub fn status(&self) -> NotificationStatus {
        NotificationStatus {
            configured: self.config.is_some(),
            server: self.config.as_ref().map(|config| config.server.clone()),
            topic: self.config.as_ref().map(|config| config.topic.clone()),
            token_configured: self
                .config
                .as_ref()
                .and_then(|config| config.token.as_ref())
                .is_some(),
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
            serde_json::to_string_pretty(&*rules).map_err(|error| error.to_string())?
        };
        if let Some(parent) = self.rules_path.parent() {
            crate::private_fs::ensure_private_dir(parent).map_err(|error| error.to_string())?;
        }
        fs::write(&self.rules_path, text).map_err(|error| {
            format!(
                "failed to write notification rules '{}': {error}",
                self.rules_path.display()
            )
        })
    }

    /// Return newest journal entries first.
    pub fn journal(&self) -> Vec<NotificationJournalEntry> {
        self.journal
            .lock()
            .map(|entries| entries.iter().rev().cloned().collect())
            .unwrap_or_default()
    }

    /// Apply rules and deduplication, then deliver one structured Event through ntfy.
    ///
    /// `capture` is the Session's capture level: an Event its capture cannot
    /// support as evidence never notifies, whatever the rules say.
    pub async fn process(
        &self,
        event: &Event,
        capture: CaptureLevel,
        machine: &str,
        label: &str,
    ) -> bool {
        if !capture.supports_evidence(&event.kind) || !self.should_send(event) {
            return false;
        }
        let text = format!("{machine} · {label}: {}", short_text(&event.kind));
        let delivered = match &self.config {
            Some(config) => {
                let url = format!("{}/{}", config.server.trim_end_matches('/'), config.topic);
                let mut request = self.client.post(url).body(text.clone());
                if let Some(token) = &config.token {
                    request = request.bearer_auth(token);
                }
                request
                    .send()
                    .await
                    .is_ok_and(|response| response.status().is_success())
            }
            None => false,
        };
        self.record(event, text, delivered);
        delivered
    }

    fn should_send(&self, event: &Event) -> bool {
        let enabled = self
            .rules
            .read()
            .ok()
            .map(|rules| {
                rules
                    .sessions
                    .get(&event.session_id)
                    .copied()
                    .unwrap_or(rules.default)
            })
            .is_some_and(|rules| category_enabled(rules, &event.kind));
        if !enabled || matches!(event.kind, EventKind::CommandFinished { .. }) {
            return enabled;
        }
        let key = (event.session_id, kind_key(&event.kind));
        let now = (self.clock)();
        let Ok(mut dedup) = self.dedup.lock() else {
            return false;
        };
        if dedup
            .get(&key)
            .is_some_and(|previous| now.0.saturating_sub(previous.0) < DEDUP_MS)
        {
            return false;
        }
        dedup.insert(key, now);
        true
    }

    fn record(&self, event: &Event, text: String, delivered: bool) {
        let Ok(mut next_id) = self.next_id.lock() else {
            return;
        };
        let entry = NotificationJournalEntry {
            id: *next_id,
            machine_id: self.machine_id.clone(),
            session_id: event.session_id,
            text,
            at: (self.clock)(),
            delivered,
        };
        *next_id = next_id.saturating_add(1);
        drop(next_id);
        if let Ok(mut journal) = self.journal.lock() {
            journal.push(entry);
            if journal.len() > MAX_JOURNAL {
                journal.remove(0);
            }
        }
    }
}

fn valid_config(config: &NotifyConfig) -> bool {
    let scheme_ok = config.server.starts_with("https://")
        || config
            .server
            .strip_prefix("http://")
            .is_some_and(private_http_host);
    scheme_ok
        && !config.server.contains('@')
        && !config.topic.is_empty()
        && config.topic.len() <= 256
        && config
            .topic
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
}

/// Plain HTTP would expose the token and event text on the wire, so it is
/// allowed only to loopback or a Tailscale (100.64.0.0/10) address, where
/// the transport is local or WireGuard-encrypted.
fn private_http_host(rest: &str) -> bool {
    let authority = rest.split('/').next().unwrap_or_default();
    let host = if let Some(bracketed) = authority.strip_prefix('[') {
        bracketed.split(']').next().unwrap_or_default()
    } else {
        authority.split(':').next().unwrap_or_default()
    };
    if host == "localhost" {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            let [a, b, ..] = ip.octets();
            ip.is_loopback() || (a == 100 && (64..128).contains(&b))
        }
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => false,
    }
}

/// The rule categories; the default set matches `EventKind::notifies_by_default`.
fn category_enabled(rules: NotificationRuleSet, kind: &EventKind) -> bool {
    match kind {
        EventKind::CommandFinished { exit_code: 0, .. } => rules.finished,
        EventKind::CommandFinished { .. } | EventKind::ErrorDetected { .. } => rules.error,
        EventKind::ResourceHigh { percent, .. } => rules.error && *percent >= 95.0,
        EventKind::PossiblyStalled { .. } => rules.stalled,
        EventKind::WaitingForInput { .. } => rules.waiting,
        EventKind::CheckpointCreated { .. } => rules.checkpoint,
        EventKind::ProgressUpdated { .. } => rules.progress,
        _ => false,
    }
}

fn kind_key(kind: &EventKind) -> &'static str {
    match kind {
        EventKind::CommandStarted { .. } => "COMMAND_STARTED",
        EventKind::CommandFinished { .. } => "COMMAND_FINISHED",
        EventKind::ProcessStarted { .. } => "PROCESS_STARTED",
        EventKind::ProcessExited { .. } => "PROCESS_EXITED",
        EventKind::ProgressUpdated { .. } => "PROGRESS_UPDATED",
        EventKind::WarningDetected { .. } => "WARNING_DETECTED",
        EventKind::ErrorDetected { .. } => "ERROR_DETECTED",
        EventKind::ResourceHigh { .. } => "RESOURCE_HIGH",
        EventKind::WaitingForInput { .. } => "WAITING_FOR_INPUT",
        EventKind::PossiblyStalled { .. } => "POSSIBLY_STALLED",
        EventKind::CheckpointCreated { .. } => "CHECKPOINT_CREATED",
        EventKind::ServerStarted { .. } => "SERVER_STARTED",
    }
}

fn short_text(kind: &EventKind) -> String {
    match kind {
        EventKind::CommandFinished { exit_code, .. } => {
            format!("command finished with exit {exit_code}")
        }
        // The message is a raw output line and may hold secrets.
        EventKind::ErrorDetected { .. } => "error detected".into(),
        EventKind::ResourceHigh { resource, percent } => {
            format!("{resource:?} usage is {percent:.0}%")
        }
        EventKind::WaitingForInput { .. } => "waiting for input".into(),
        EventKind::PossiblyStalled { silent_for_ms } => {
            format!("possibly stalled for {} seconds", silent_for_ms.0 / 1_000)
        }
        EventKind::CheckpointCreated { path } => format!("checkpoint created: {path}"),
        EventKind::ProgressUpdated {
            current,
            total,
            unit,
        } => format!(
            "progress: {current}{} {unit}",
            total.map_or_else(String::new, |total| format!("/{total}"))
        ),
        _ => "needs attention".into(),
    }
}

/// Deliver every Session Event through `service`, labelled with the Session
/// title or its latest Command text, until the Event stream closes.
pub fn spawn_notifier(
    service: NotificationService,
    mut events: tokio::sync::broadcast::Receiver<Event>,
    store: Arc<store::Store>,
    machine: String,
) {
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    // A Session the store does not know has no evidence to report.
                    let Some(session) = store.session(event.session_id).ok().flatten() else {
                        continue;
                    };
                    let label = session
                        .title
                        .or_else(|| {
                            store
                                .history(Some(event.session_id), 1)
                                .ok()
                                .and_then(|commands| commands.into_iter().next())
                                .map(|command| command.text)
                        })
                        .unwrap_or_else(|| format!("Session {}", event.session_id.0));
                    let _ = service
                        .process(&event, session.capture, &machine, &label)
                        .await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!(skipped, "notification Event stream lagged");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests;
