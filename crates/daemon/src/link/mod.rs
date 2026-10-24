//! Worker side of a Machine link: identity, pairing, heartbeats, and revocation.
//!
//! The worker always dials out to its control. It keeps the plaintext worker
//! credential it presents and only a SHA-256 of the control credential it
//! verifies. The pairing code passes through once, in one request body.

mod approval;
mod client;
mod file;
mod handoff;

use crate::{
    control::all_capabilities,
    identity::{self, MachineIdentity},
    link_audit::{LinkAudit, LinkAuditRecord},
    link_crypto,
};
use client::{CallError, ControlClient};
use file::{LinkFile, StoredState};
use protocol::{
    link::{
        normalize_pairing_code, validate_control_url, validate_display_name, ConnectionReport,
        ControlUrlPolicy, LinkDisconnectRequest, LinkEndpoint, LinkExchangeRequest,
        LinkExchangeResponse, LinkFailureCode, LinkHeartbeatRequest, LinkHeartbeatResponse,
        LinkIdentity, LinkMachineInfo, LinkRefreshRequest, LinkRefreshResponse, LocalLinkState,
    },
    LocalErrorCode, LocalRequest, LocalResponse, MachineId, NotificationDeliveryState,
    NotificationIntentAcknowledgement, NotificationIntentBatch, NotificationResultClass, Os,
    ProjectionAcknowledgement, ProjectionBatch, ProjectionSyncAction, ProjectionSyncDirective,
    Secret, Timestamp,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use store::Store;

/// Consecutive failed heartbeats before the control is reported unreachable.
const UNREACHABLE_AFTER: u32 = 3;

type Clock = Arc<dyn Fn() -> Timestamp + Send + Sync>;

/// Static settings of this daemon's link role.
pub struct LinkSettings {
    /// Owner-only link file.
    pub link_file: PathBuf,
    /// Identity file (for an explicit reset).
    pub identity_file: PathBuf,
    /// Identity loaded at startup, or why it is unusable.
    pub identity: Result<MachineIdentity, String>,
    /// Pre-4.0 hostname ID, recorded in a reset identity.
    pub legacy_machine_id: String,
    /// Hostname, the default display name.
    pub hostname: String,
    /// Where the control can reach this daemon.
    pub endpoint: LinkEndpoint,
    /// This Machine's OS.
    pub os: Os,
    /// This daemon's version.
    pub daemon_version: String,
    /// Development mode: plain-HTTP controls on loopback/Tailscale IPs.
    pub development: bool,
    /// Directory for `link-audit.jsonl`.
    pub audit_dir: PathBuf,
    /// Run heartbeats on a timer (tests drive them instead).
    pub background: bool,
}

/// Start the durable worker native-notification outbox uploader.
pub fn spawn_notification_intent_uploader(
    link: LinkManager,
    store: Arc<Store>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = interval.tick() => {
                    if !link.notification_ingress() {
                        continue;
                    }
                    let now = (link.inner.clock)();
                    let due = match store.due_notification_intents(now, 32) {
                        Ok(due) => due,
                        Err(error) => {
                            tracing::warn!(%error, "native notification outbox scan failed");
                            continue;
                        }
                    };
                    if due.is_empty() {
                        continue;
                    }
                    let machine_id = match link.machine_id() {
                        Ok(machine_id) => machine_id,
                        Err(error) => {
                            tracing::warn!(%error, "native notification identity unavailable");
                            continue;
                        }
                    };
                    let batch = NotificationIntentBatch {
                        machine_id,
                        sent_at: now,
                        intents: due.iter().map(|row| row.intent.clone()).collect(),
                    };
                    match link.upload_notification_intents(&batch).await {
                        Ok(acknowledgement) => {
                            let mut resolved = Vec::new();
                            for intent_id in acknowledgement
                                .accepted
                                .iter()
                                .chain(acknowledgement.duplicates.iter())
                            {
                                if let Some(row) = due.iter().find(|row| row.intent.intent_id == *intent_id) {
                                    if let Err(error) = store.update_notification_delivery_for_event(
                                        row.intent.event_id,
                                        NotificationDeliveryState::ControlAccepted,
                                        NotificationResultClass::ControlAccepted,
                                        now,
                                    ) {
                                        tracing::warn!(%error, "native notification journal acknowledgement was not persisted");
                                    }
                                }
                                resolved.push(intent_id.clone());
                            }
                            for rejection in acknowledgement.rejected {
                                if let Some(row) = due.iter().find(|row| row.intent.intent_id == rejection.intent_id) {
                                    if let Err(error) = store.update_notification_delivery_for_event(
                                        row.intent.event_id,
                                        NotificationDeliveryState::PermanentlyFailed,
                                        NotificationResultClass::ClientError,
                                        now,
                                    ) {
                                        tracing::warn!(%error, "native notification rejection was not persisted");
                                    }
                                }
                                resolved.push(rejection.intent_id);
                            }
                            if let Err(error) = store.acknowledge_notification_intents(&resolved) {
                                tracing::warn!(%error, "native notification acknowledgement was not persisted");
                            }
                        }
                        Err(error) => {
                            for row in due {
                                let attempts = row.attempts.saturating_add(1);
                                let exponent = attempts.saturating_sub(1).min(6);
                                let delay = 5_000_i64.saturating_mul(1_i64 << exponent);
                                let next = Timestamp(now.0.saturating_add(delay.min(300_000)));
                                if let Err(store_error) = store.reschedule_notification_intent(
                                    &row.intent.intent_id,
                                    attempts,
                                    next,
                                    "CONTROL_UNREACHABLE",
                                    now,
                                ) {
                                    tracing::warn!(%store_error, "native notification retry was not persisted");
                                }
                            }
                            tracing::warn!(code = ?error.code, "native notification upload failed");
                        }
                    }
                }
            }
        }
    });
}

/// Start the durable worker projection uploader negotiated by heartbeats.
pub fn spawn_projection_uploader(
    link: LinkManager,
    store: Arc<Store>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = interval.tick() => {
                    if let Err(error) = upload_projection_once(&link, &store).await {
                        crate::self_health::COUNTERS.upload_failed();
                        tracing::warn!(message = %error.message, "projection upload failed");
                    }
                }
            }
        }
    });
}

async fn upload_projection_once(link: &LinkManager, store: &Store) -> Result<(), LinkFailure> {
    let Some(directive) = link.projection_directive()? else {
        return Ok(());
    };
    let machine_id = link.machine_id().map_err(identity_failure)?;
    let epoch = projection_epoch(&machine_id);
    if directive.action == ProjectionSyncAction::Snapshot {
        let report = store
            .projection_sync_report(&machine_id, &epoch)
            .map_err(internal)?;
        let records = store
            .worker_projection_snapshot(&machine_id, &epoch)
            .map_err(internal)?;
        let batches = projection_snapshot_batches(
            &machine_id,
            &epoch,
            report.current_cursor,
            (link.inner.clock)(),
            records,
            usize::from(directive.max_changes),
            directive.max_batch_bytes as usize,
        )?;
        for batch in batches {
            let acknowledgement = link.upload_projection(&batch).await?;
            if acknowledgement.status == protocol::ProjectionAcknowledgementStatus::ResyncRequired {
                return Ok(());
            }
        }
        if report.current_cursor > 0 {
            store
                .acknowledge_projection_cursor(&machine_id, &epoch, report.current_cursor)
                .map_err(internal)?;
        }
        return Ok(());
    }
    if directive.action != ProjectionSyncAction::Upload {
        return Ok(());
    }
    let changes = store
        .projection_changes_after(
            &machine_id,
            &epoch,
            directive.committed_cursor,
            u32::from(directive.max_changes),
        )
        .map_err(internal)?;
    let Some(through_cursor) = changes.last().map(protocol::ProjectionChange::cursor) else {
        return Ok(());
    };
    let oldest = store
        .oldest_projection_change_at(&machine_id, &epoch, directive.committed_cursor)
        .ok()
        .flatten();
    let batch = ProjectionBatch::Delta {
        schema_version: protocol::PROJECTION_SCHEMA_VERSION,
        machine_id: machine_id.clone(),
        source_epoch: epoch.clone(),
        base_cursor: directive.committed_cursor,
        through_cursor,
        sent_at: (link.inner.clock)(),
        changes,
    };
    let acknowledgement = link.upload_projection(&batch).await?;
    match acknowledgement.status {
        protocol::ProjectionAcknowledgementStatus::Accepted
        | protocol::ProjectionAcknowledgementStatus::Duplicate => {
            if let Some(oldest) = oldest {
                crate::self_health::COUNTERS.upload_delay((link.inner.clock)().0 - oldest.0);
            }
            store
                .acknowledge_projection_cursor(
                    &machine_id,
                    &epoch,
                    acknowledgement.committed_cursor,
                )
                .map_err(internal)?;
        }
        protocol::ProjectionAcknowledgementStatus::ResyncRequired => {}
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn projection_snapshot_batches(
    machine_id: &MachineId,
    source_epoch: &str,
    snapshot_cursor: u64,
    sent_at: Timestamp,
    records: Vec<protocol::ProjectionChange>,
    maximum_changes: usize,
    maximum_bytes: usize,
) -> Result<Vec<ProjectionBatch>, LinkFailure> {
    if maximum_changes == 0 || maximum_bytes == 0 {
        return Err(internal(
            "control advertised an empty projection snapshot bound",
        ));
    }
    if records.is_empty() {
        return Ok(vec![ProjectionBatch::Snapshot {
            schema_version: protocol::PROJECTION_SCHEMA_VERSION,
            machine_id: machine_id.clone(),
            source_epoch: source_epoch.into(),
            snapshot_id: format!("ps-{snapshot_cursor}"),
            snapshot_cursor,
            chunk_index: 0,
            final_chunk: true,
            sent_at,
            records,
        }]);
    }
    let snapshot_id = format!("ps-{snapshot_cursor}");
    let mut batches = Vec::new();
    let mut offset = 0;
    while offset < records.len() {
        let mut take = maximum_changes.min(records.len() - offset);
        loop {
            let final_chunk = offset + take == records.len();
            let batch = ProjectionBatch::Snapshot {
                schema_version: protocol::PROJECTION_SCHEMA_VERSION,
                machine_id: machine_id.clone(),
                source_epoch: source_epoch.into(),
                snapshot_id: snapshot_id.clone(),
                snapshot_cursor,
                chunk_index: batches.len() as u32,
                final_chunk,
                sent_at,
                records: records[offset..offset + take].to_vec(),
            };
            let encoded = serde_json::to_vec(&batch).map_err(internal)?;
            if encoded.len() <= maximum_bytes {
                batches.push(batch);
                offset += take;
                break;
            }
            if take == 1 {
                return Err(internal(format!(
                    "one projection snapshot record exceeds the {maximum_bytes}-byte control bound"
                )));
            }
            take -= 1;
        }
    }
    Ok(batches)
}

pub(crate) fn projection_epoch(machine_id: &MachineId) -> String {
    format!("projection-v1-{}", machine_id.0)
}

/// A link operation that failed, mapped to a local-socket error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkFailure {
    /// Stable category for CLI exit codes.
    pub code: LocalErrorCode,
    /// Detail for the owner; never contains a secret.
    pub message: String,
}

impl LinkFailure {
    fn new(code: LocalErrorCode, message: impl Into<String>) -> Self {
        LinkFailure {
            code,
            message: message.into(),
        }
    }
}

struct Inner {
    settings: LinkSettings,
    clock: Clock,
    client: ControlClient,
    identity: Mutex<Result<MachineIdentity, String>>,
    /// Serializes connect, heartbeat, refresh, and disconnect.
    operation: tokio::sync::Mutex<()>,
    failures: AtomicU32,
    heartbeats_running: AtomicBool,
    projection_store: Mutex<Option<Arc<Store>>>,
    projection_directive: Mutex<Option<ProjectionSyncDirective>>,
    approval: Mutex<Option<approval::PendingApproval>>,
    updater: Mutex<Option<crate::worker_update::WorkerUpdater>>,
    /// The latest health report not yet delivered by a heartbeat (4.H).
    health: Mutex<Option<protocol::observability::DaemonHealthReport>>,
}

/// This daemon's link to an owner-selected control Machine.
#[derive(Clone)]
pub struct LinkManager {
    inner: Arc<Inner>,
}

impl LinkManager {
    /// Create the manager; starts heartbeats when an active link exists.
    pub fn open(settings: LinkSettings, clock: Clock) -> Self {
        let manager = LinkManager {
            inner: Arc::new(Inner {
                client: ControlClient::new(settings.development),
                identity: Mutex::new(settings.identity.clone()),
                settings,
                clock,
                operation: tokio::sync::Mutex::new(()),
                failures: AtomicU32::new(0),
                heartbeats_running: AtomicBool::new(false),
                projection_store: Mutex::new(None),
                projection_directive: Mutex::new(None),
                approval: Mutex::new(None),
                updater: Mutex::new(None),
                health: Mutex::new(None),
            }),
        };
        if matches!(manager.read_link(), Ok(Some(link)) if link.state == StoredState::Active) {
            manager.spawn_heartbeats();
        }
        manager
    }

    /// Report this worker's dashboard-started updates in heartbeats.
    pub fn with_updater(self, updater: crate::worker_update::WorkerUpdater) -> Self {
        if let Ok(mut slot) = self.inner.updater.lock() {
            *slot = Some(updater);
        }
        self
    }

    /// Attach the local Store used for projection heartbeat negotiation.
    pub fn with_projection_store(self, store: Arc<Store>) -> Self {
        if let Ok(mut configured) = self.inner.projection_store.lock() {
            *configured = Some(store);
        } else {
            tracing::error!("projection Store lock poisoned");
        }
        self
    }

    /// The immutable Machine ID, unless the identity is corrupt.
    pub fn machine_id(&self) -> Result<MachineId, String> {
        self.inner
            .identity
            .lock()
            .map_err(|_| "identity lock poisoned".to_string())?
            .as_ref()
            .map(|identity| identity.machine_id.clone())
            .map_err(Clone::clone)
    }

    /// Whether the last authenticated heartbeat enabled native intent ingress.
    pub fn notification_ingress(&self) -> bool {
        self.read_link()
            .ok()
            .flatten()
            .is_some_and(|link| link.state == StoredState::Active && link.notification_ingress)
    }

    /// Upload one bounded intent batch using the existing Machine credential.
    pub async fn upload_notification_intents(
        &self,
        batch: &NotificationIntentBatch,
    ) -> Result<NotificationIntentAcknowledgement, LinkFailure> {
        let link = self
            .read_link()?
            .ok_or_else(|| LinkFailure::new(LocalErrorCode::NotConnected, "not connected"))?;
        let credential = link
            .worker_credential
            .as_ref()
            .filter(|_| link.state == StoredState::Active && link.notification_ingress)
            .ok_or_else(|| {
                LinkFailure::new(
                    LocalErrorCode::NotConnected,
                    "native notification ingress is unavailable",
                )
            })?;
        self.inner
            .client
            .post(
                &link.control_url,
                "/api/v1/link/notification-intents",
                Some(credential.expose()),
                batch,
            )
            .await
            .map_err(|error| {
                LinkFailure::new(LocalErrorCode::ControlUnreachable, error.to_string())
            })?
            .ok_or_else(|| {
                LinkFailure::new(
                    LocalErrorCode::ControlUnreachable,
                    "empty notification acknowledgement",
                )
            })
    }

    /// Upload one bounded projection batch using the Machine link credential.
    pub async fn upload_projection(
        &self,
        batch: &ProjectionBatch,
    ) -> Result<ProjectionAcknowledgement, LinkFailure> {
        let link = self
            .read_link()?
            .ok_or_else(|| LinkFailure::new(LocalErrorCode::NotConnected, "not connected"))?;
        let credential = link
            .worker_credential
            .as_ref()
            .filter(|_| link.state == StoredState::Active)
            .ok_or_else(|| LinkFailure::new(LocalErrorCode::NotConnected, "not connected"))?;
        self.inner
            .client
            .post(
                &link.control_url,
                "/api/v1/link/projection",
                Some(credential.expose()),
                batch,
            )
            .await
            .map_err(|error| {
                LinkFailure::new(LocalErrorCode::ControlUnreachable, error.to_string())
            })?
            .ok_or_else(|| {
                LinkFailure::new(
                    LocalErrorCode::ControlUnreachable,
                    "empty projection acknowledgement",
                )
            })
    }

    fn projection_directive(&self) -> Result<Option<ProjectionSyncDirective>, LinkFailure> {
        self.inner
            .projection_directive
            .lock()
            .map(|value| value.clone())
            .map_err(|_| internal("projection directive lock poisoned"))
    }

    /// Answer one local-socket link request.
    pub async fn handle_local(&self, request: LocalRequest) -> LocalResponse {
        let result = match request {
            LocalRequest::GetIdentity => self.identity_response(),
            LocalRequest::ConnectControl {
                control_url,
                pairing_code,
                display_name,
            } => self
                .connect(&control_url, &pairing_code, display_name.as_deref())
                .await
                .map(|connection| LocalResponse::Connection { connection }),
            LocalRequest::ConnectionStatus => self
                .status()
                .map(|connection| LocalResponse::Connection { connection }),
            LocalRequest::DisconnectControl => self
                .disconnect()
                .await
                .map(|connection| LocalResponse::Connection { connection }),
            LocalRequest::ResetIdentity => self.reset_identity().await,
            LocalRequest::RequestApproval {
                control_url,
                control_machine_id,
                display_name,
                keep_current_link,
            } => {
                self.request_approval(
                    &control_url,
                    control_machine_id,
                    display_name.as_deref(),
                    keep_current_link,
                )
                .await
            }
            LocalRequest::ClaimApproval => self.claim_approval().await,
            LocalRequest::CancelApproval => self.cancel_approval().await,
            _ => Err(LinkFailure::new(
                LocalErrorCode::Unavailable,
                "not a Machine link request",
            )),
        };
        result.unwrap_or_else(|failure| LocalResponse::Error {
            code: failure.code,
            message: failure.message,
        })
    }

    /// Pair with a control using a one-time code.
    pub async fn connect(
        &self,
        control_url: &str,
        pairing_code: &Secret,
        display_name: Option<&str>,
    ) -> Result<ConnectionReport, LinkFailure> {
        let _operation = self.inner.operation.lock().await;
        let machine_id = self.machine_id().map_err(identity_failure)?;
        let now = (self.inner.clock)();
        if let Some(link) = self.read_link()? {
            if link.state == StoredState::Active {
                return Err(LinkFailure::new(
                    LocalErrorCode::AlreadyConnected,
                    format!(
                        "this Machine ({machine_id}) is already connected to {}; run `sentinal disconnect` first",
                        link.control_url
                    ),
                ));
            }
        }
        let policy = if self.inner.settings.development {
            ControlUrlPolicy::Development
        } else {
            ControlUrlPolicy::Production
        };
        let control_url = validate_control_url(control_url, policy)
            .map_err(|error| LinkFailure::new(LocalErrorCode::InvalidRequest, error.to_string()))?;
        normalize_pairing_code(pairing_code.expose())
            .map_err(|error| LinkFailure::new(LocalErrorCode::CodeRejected, error.to_string()))?;
        let display_name = validate_display_name(
            display_name.unwrap_or(&self.inner.settings.hostname),
        )
        .map_err(|error| LinkFailure::new(LocalErrorCode::InvalidRequest, error.to_string()))?;
        let control_credential = link_crypto::new_credential()
            .map_err(|error| LinkFailure::new(LocalErrorCode::Internal, error.to_string()))?;
        let request = LinkExchangeRequest {
            pairing_code: pairing_code.clone(),
            machine: LinkMachineInfo {
                machine_id: machine_id.clone(),
                display_name,
                os: self.inner.settings.os,
                daemon_version: self.inner.settings.daemon_version.clone(),
                capabilities: all_capabilities(),
            },
            endpoint: self.inner.settings.endpoint.clone(),
            control_credential: Secret::new(control_credential.clone()),
        };
        let response: LinkExchangeResponse = match self
            .inner
            .client
            .post(&control_url, "/api/v1/link/exchange", None, &request)
            .await
        {
            Ok(Some(response)) => response,
            Ok(None) => {
                return Err(self.connect_failed(
                    now,
                    "empty exchange response",
                    LocalErrorCode::ControlUnreachable,
                ))
            }
            Err(error) => {
                let code = match error.code() {
                    Some(LinkFailureCode::InvalidCode | LinkFailureCode::PairingLocked) => {
                        LocalErrorCode::CodeRejected
                    }
                    Some(LinkFailureCode::InvalidMachine | LinkFailureCode::InvalidRequest) => {
                        LocalErrorCode::InvalidRequest
                    }
                    _ => LocalErrorCode::ControlUnreachable,
                };
                let message = match (&error, error.code()) {
                    (_, Some(LinkFailureCode::NotControl)) | (CallError::Unexpected(_), _) => {
                        format!("{control_url} is not a control Machine")
                    }
                    (_, Some(LinkFailureCode::InvalidCode)) => {
                        "that pairing code is not valid (wrong, expired, or already used); create a new one in Settings → Machines".into()
                    }
                    (_, Some(LinkFailureCode::PairingLocked)) => {
                        "the control locked pairing after too many wrong codes; wait 10 minutes and create a new code".into()
                    }
                    (CallError::Refused { failure, .. }, _) => failure.message.clone(),
                    (CallError::Unreachable(detail), _) => format!("cannot reach {control_url}: {detail}"),
                };
                return Err(self.connect_failed(now, &message, code));
            }
        };
        let link = LinkFile::active(
            control_url,
            response.control_machine_id,
            response.link_id,
            response.worker_credential,
            &control_credential,
            response.credential_expires_at,
            response.display_name,
            now,
            response.heartbeat_interval_ms,
        );
        self.write_link(&link)?;
        self.inner.failures.store(0, Ordering::SeqCst);
        self.audit_link(now, "CONNECTED", link.link_id.as_deref(), None);
        drop(_operation);
        self.spawn_heartbeats();
        self.report(Some(&link), &machine_id)
    }

    /// The current link as seen locally.
    pub fn status(&self) -> Result<ConnectionReport, LinkFailure> {
        let machine_id = self.machine_id().map_err(identity_failure)?;
        let link = self.read_link()?;
        self.report(link.as_ref(), &machine_id)
    }

    /// Remove the local link, telling the control when it is reachable.
    pub async fn disconnect(&self) -> Result<ConnectionReport, LinkFailure> {
        let _operation = self.inner.operation.lock().await;
        let machine_id = self.machine_id().map_err(identity_failure)?;
        let now = (self.inner.clock)();
        let Some(link) = self.read_link()? else {
            return Err(LinkFailure::new(
                LocalErrorCode::NotConnected,
                "this Machine is not connected to a control",
            ));
        };
        let mut message = None;
        if let (StoredState::Active, Some(credential)) = (link.state, &link.worker_credential) {
            let told = self
                .inner
                .client
                .post::<_, serde_json::Value>(
                    &link.control_url,
                    "/api/v1/link/disconnect",
                    Some(credential.expose()),
                    &LinkDisconnectRequest {
                        machine_id: machine_id.clone(),
                    },
                )
                .await;
            if let Err(error) = told {
                message = Some(format!(
                    "the control was not told ({error}); revoke {machine_id} in Settings → Machines"
                ));
            }
        }
        file::remove(&self.inner.settings.link_file).map_err(internal)?;
        self.inner.failures.store(0, Ordering::SeqCst);
        self.audit_link(
            now,
            "DISCONNECTED",
            link.link_id.as_deref(),
            message.as_deref(),
        );
        let mut report = self.report(None, &machine_id)?;
        report.message = message;
        Ok(report)
    }

    /// Queue this daemon's latest health report for the next heartbeat; an
    /// undelivered earlier report's counts are folded in.
    pub fn set_health_report(&self, report: protocol::observability::DaemonHealthReport) {
        if let Ok(mut pending) = self.inner.health.lock() {
            *pending = Some(crate::self_health::merge_unsent(pending.take(), report));
        }
    }

    /// Send one heartbeat, refreshing credentials when the control asks.
    pub async fn heartbeat_once(&self) -> Result<(), LinkFailure> {
        let _operation = self.inner.operation.lock().await;
        let machine_id = self.machine_id().map_err(identity_failure)?;
        let Some(mut link) = self.read_link()? else {
            return Err(LinkFailure::new(
                LocalErrorCode::NotConnected,
                "not connected",
            ));
        };
        let Some(credential) = link
            .worker_credential
            .clone()
            .filter(|_| link.state == StoredState::Active)
        else {
            return Err(LinkFailure::new(
                LocalErrorCode::NotConnected,
                "not connected",
            ));
        };
        let now = (self.inner.clock)();
        // Persist the sequence before sending so a crash can never reuse it.
        link.sequence += 1;
        self.write_link(&link)?;
        let projection_sync = self
            .inner
            .projection_store
            .lock()
            .map_err(|_| internal("projection Store lock poisoned"))?
            .as_ref()
            .map(|store| store.projection_sync_report(&machine_id, &projection_epoch(&machine_id)))
            .transpose()
            .map_err(internal)?;
        let request = LinkHeartbeatRequest {
            machine_id: machine_id.clone(),
            sequence: link.sequence,
            sent_at: now,
            endpoint: self.inner.settings.endpoint.clone(),
            daemon_version: self.inner.settings.daemon_version.clone(),
            capabilities: all_capabilities(),
            projection_sync,
            update_status: self
                .inner
                .updater
                .lock()
                .ok()
                .and_then(|updater| updater.as_ref().and_then(|updater| updater.status())),
            health: self
                .inner
                .health
                .lock()
                .ok()
                .and_then(|pending| pending.clone()),
        };
        let result: Result<Option<LinkHeartbeatResponse>, CallError> = self
            .inner
            .client
            .post(
                &link.control_url,
                "/api/v1/link/heartbeat",
                Some(credential.expose()),
                &request,
            )
            .await;
        match result {
            Ok(Some(response)) => {
                self.inner.failures.store(0, Ordering::SeqCst);
                // Delivered: a newer report queued meanwhile is kept.
                if let Ok(mut pending) = self.inner.health.lock() {
                    if pending.as_ref() == request.health.as_ref() {
                        *pending = None;
                    }
                }
                link.display_name = Some(response.display_name);
                link.last_heartbeat_at = Some(now);
                link.heartbeat_interval_ms = response.next_heartbeat_ms;
                link.notification_ingress = response.notification_ingress.unwrap_or(false);
                *self
                    .inner
                    .projection_directive
                    .lock()
                    .map_err(|_| internal("projection directive lock poisoned"))? =
                    response.projection_sync;
                self.write_link(&link)?;
                if response.refresh_due {
                    self.refresh(&mut link, &machine_id, &credential).await?;
                }
                Ok(())
            }
            Err(error) if error.code() == Some(LinkFailureCode::LinkRevoked) => {
                link.revoke();
                self.write_link(&link)?;
                self.audit_link(now, "REVOKED", None, None);
                Err(LinkFailure::new(
                    LocalErrorCode::NotConnected,
                    "the control revoked this Machine",
                ))
            }
            other => {
                let detail = match other {
                    Err(error) => error.to_string(),
                    Ok(None) => "empty heartbeat response".into(),
                    Ok(Some(_)) => unreachable!("handled above"),
                };
                let failures = self.inner.failures.fetch_add(1, Ordering::SeqCst) + 1;
                tracing::warn!(failures, error = %detail, "control heartbeat failed");
                crate::self_health::COUNTERS
                    .error(protocol::observability::HealthLabel::ErrorHeartbeat);
                Err(LinkFailure::new(LocalErrorCode::ControlUnreachable, detail))
            }
        }
    }

    /// Identity proof for `GET /api/v1/link/identity`, given the control's bearer.
    pub fn verify_control_credential(&self, bearer: &str) -> Option<LinkIdentity> {
        let link = self.read_link().ok().flatten()?;
        let expected = link.control_credential_sha256.as_deref()?;
        let expires_at = link.credential_expires_at?;
        if link.state != StoredState::Active
            || expires_at.0 <= (self.inner.clock)().0
            || !link_crypto::same_hash(&link_crypto::sha256_hex(bearer), expected)
        {
            return None;
        }
        Some(LinkIdentity {
            machine_id: self.machine_id().ok()?,
            control_machine_id: link.control_machine_id?,
        })
    }

    async fn refresh(
        &self,
        link: &mut LinkFile,
        machine_id: &MachineId,
        credential: &Secret,
    ) -> Result<(), LinkFailure> {
        let control_credential = link_crypto::new_credential().map_err(internal)?;
        let response: LinkRefreshResponse = self
            .inner
            .client
            .post(
                &link.control_url,
                "/api/v1/link/refresh",
                Some(credential.expose()),
                &LinkRefreshRequest {
                    machine_id: machine_id.clone(),
                    control_credential: Secret::new(control_credential.clone()),
                },
            )
            .await
            .map_err(|error| {
                LinkFailure::new(LocalErrorCode::ControlUnreachable, error.to_string())
            })?
            .ok_or_else(|| {
                LinkFailure::new(LocalErrorCode::ControlUnreachable, "empty refresh response")
            })?;
        link.worker_credential = Some(response.worker_credential);
        link.control_credential_sha256 = Some(link_crypto::sha256_hex(&control_credential));
        link.credential_expires_at = Some(response.credential_expires_at);
        self.write_link(link)?;
        self.audit_link(
            (self.inner.clock)(),
            "CREDENTIAL_REFRESHED",
            link.link_id.as_deref(),
            None,
        );
        Ok(())
    }

    async fn reset_identity(&self) -> Result<LocalResponse, LinkFailure> {
        let _operation = self.inner.operation.lock().await;
        let now = (self.inner.clock)();
        let identity = identity::reset(
            &self.inner.settings.identity_file,
            now,
            &self.inner.settings.legacy_machine_id,
        )
        .map_err(|error| LinkFailure::new(LocalErrorCode::Internal, error.to_string()))?;
        file::remove(&self.inner.settings.link_file).map_err(internal)?;
        let machine_id = identity.machine_id.clone();
        *self
            .inner
            .identity
            .lock()
            .map_err(|_| internal("identity lock poisoned"))? = Ok(identity);
        self.audit_link(now, "IDENTITY_RESET", None, Some(&machine_id.0));
        Ok(LocalResponse::Identity {
            machine_id,
            hostname: self.inner.settings.hostname.clone(),
        })
    }

    fn identity_response(&self) -> Result<LocalResponse, LinkFailure> {
        Ok(LocalResponse::Identity {
            machine_id: self.machine_id().map_err(identity_failure)?,
            hostname: self.inner.settings.hostname.clone(),
        })
    }

    fn report(
        &self,
        link: Option<&LinkFile>,
        machine_id: &MachineId,
    ) -> Result<ConnectionReport, LinkFailure> {
        let Some(link) = link else {
            return Ok(ConnectionReport {
                state: LocalLinkState::NotConnected,
                machine_id: machine_id.clone(),
                control_url: None,
                control_machine_id: None,
                display_name: None,
                link_id: None,
                paired_at: None,
                last_heartbeat_at: None,
                message: None,
            });
        };
        let failures = self.inner.failures.load(Ordering::SeqCst);
        let (state, message) = match link.state {
            StoredState::Revoked => (
                LocalLinkState::Revoked,
                Some("the control revoked this Machine".to_string()),
            ),
            StoredState::Active if failures >= UNREACHABLE_AFTER => (
                LocalLinkState::ControlUnreachable,
                Some(format!("{failures} heartbeats in a row failed")),
            ),
            StoredState::Active => (LocalLinkState::Active, None),
        };
        Ok(ConnectionReport {
            state,
            machine_id: machine_id.clone(),
            control_url: Some(link.control_url.clone()),
            control_machine_id: link.control_machine_id.clone(),
            display_name: link.display_name.clone(),
            link_id: link.link_id.clone(),
            paired_at: link.paired_at,
            last_heartbeat_at: link.last_heartbeat_at,
            message,
        })
    }

    fn spawn_heartbeats(&self) {
        if !self.inner.settings.background
            || self.inner.heartbeats_running.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            self.inner.heartbeats_running.store(false, Ordering::SeqCst);
            return;
        };
        let manager = self.clone();
        runtime.spawn(async move {
            loop {
                let _ = manager.heartbeat_once().await;
                let link = manager.read_link().ok().flatten();
                let Some(link) = link.filter(|link| link.state == StoredState::Active) else {
                    manager
                        .inner
                        .heartbeats_running
                        .store(false, Ordering::SeqCst);
                    return;
                };
                tokio::time::sleep(jittered(link.heartbeat_interval_ms)).await;
            }
        });
    }

    fn connect_failed(&self, now: Timestamp, message: &str, code: LocalErrorCode) -> LinkFailure {
        let reason = format!("{code:?}");
        self.audit_link(now, "CONNECT_FAILED", None, Some(&reason));
        LinkFailure::new(code, message)
    }

    fn audit_link(&self, at: Timestamp, kind: &str, link_id: Option<&str>, reason: Option<&str>) {
        let machine_id = self.machine_id().ok();
        LinkAudit::new(&self.inner.settings.audit_dir, "link-audit.jsonl").append(
            &LinkAuditRecord {
                machine_id: machine_id.as_ref().map(|id| id.0.as_str()),
                link_id,
                reason,
                ..LinkAuditRecord::new(at, kind)
            },
        );
    }

    fn read_link(&self) -> Result<Option<LinkFile>, LinkFailure> {
        file::read(&self.inner.settings.link_file).map_err(internal)
    }

    fn write_link(&self, link: &LinkFile) -> Result<(), LinkFailure> {
        file::write(&self.inner.settings.link_file, link).map_err(internal)
    }
}

/// ±10 % jitter so many workers do not heartbeat in lockstep.
fn jittered(interval_ms: u64) -> Duration {
    let byte = crate::auth::secure_random::<1>()
        .map(|bytes| bytes[0])
        .unwrap_or(128);
    let spread = interval_ms / 10;
    let offset = spread * u64::from(byte) * 2 / 255;
    Duration::from_millis(interval_ms.saturating_sub(spread) + offset)
}

fn identity_failure(message: String) -> LinkFailure {
    LinkFailure::new(LocalErrorCode::IdentityCorrupt, message)
}

fn internal(message: impl std::fmt::Display) -> LinkFailure {
    LinkFailure::new(LocalErrorCode::Internal, message.to_string())
}

#[cfg(test)]
mod projection_tests {
    use super::*;
    use protocol::{ProjectedMetricScope, ProjectedMetrics, ProjectionChange, SessionId};

    fn metric(cursor: u64) -> ProjectionChange {
        ProjectionChange::MetricsReplace {
            cursor,
            metrics: ProjectedMetrics {
                scope: ProjectedMetricScope::Session,
                session_id: Some(SessionId(cursor)),
                at: Timestamp(cursor as i64),
                cpu_percent: cursor as f32,
                ram_used_bytes: cursor,
                ram_total_bytes: None,
                gpu_percent: None,
                vram_used_bytes: None,
                vram_total_bytes: None,
            },
        }
    }

    #[test]
    fn snapshot_batches_are_bounded_ordered_and_finalized_once() {
        let batches = projection_snapshot_batches(
            &MachineId("machine-a".into()),
            "epoch-a",
            3,
            Timestamp(10),
            vec![metric(1), metric(2), metric(3)],
            2,
            64 * 1024,
        )
        .unwrap();
        assert_eq!(batches.len(), 2);
        assert!(matches!(
            &batches[0],
            ProjectionBatch::Snapshot {
                chunk_index: 0,
                final_chunk: false,
                records,
                ..
            } if records.len() == 2
        ));
        assert!(matches!(
            &batches[1],
            ProjectionBatch::Snapshot {
                chunk_index: 1,
                final_chunk: true,
                records,
                ..
            } if records.len() == 1
        ));
    }

    #[test]
    fn empty_projection_has_one_final_snapshot_chunk() {
        let batches = projection_snapshot_batches(
            &MachineId("machine-a".into()),
            "epoch-a",
            0,
            Timestamp(10),
            vec![],
            128,
            64 * 1024,
        )
        .unwrap();
        assert!(matches!(
            &batches[0],
            ProjectionBatch::Snapshot {
                chunk_index: 0,
                final_chunk: true,
                records,
                ..
            } if records.is_empty()
        ));
    }
}
