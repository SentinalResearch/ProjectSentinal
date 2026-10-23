//! Owner-selected control Machine: pairing codes, the registry, and worker links.
//!
//! Codes are 50-bit, single-use, 10-minute secrets stored only as keyed
//! HMACs. A successful exchange creates one link with two directional
//! credentials; each side keeps only a hash of the one it verifies.

mod approval;
mod approval_claim;
mod approval_routes;
mod approval_verify;
mod discovery;
mod handoff;
mod ping;
mod proxy;
mod refresh;
mod release;
pub mod release_actions;
mod release_routes;
pub mod release_watch;
pub mod routes;
mod secrets;
mod worker_update;

pub use approval_verify::{verification_words, PairingVerifier, TailnetVerifier, VerifyFuture};
pub use refresh::RefreshFault;

pub use release::{ControlRelease, ControlReleaseSettings, Detach};

use crate::{
    link_audit::{LinkAudit, LinkAuditRecord},
    link_crypto,
};
use protocol::{
    link::{
        connect_command, normalize_pairing_code, validate_display_name, LinkCredentialInfo,
        LinkEndpoint, LinkExchangeRequest, LinkExchangeResponse, LinkFailure, LinkFailureCode,
        LinkHeartbeatRequest, LinkHeartbeatResponse, LinkStatus, MachineCapability, MachineRole,
        PairingCode, PairingCodeState, PairingCodeStatus, RegistryMachine, RegistryMachines,
        RegistryStatus,
    },
    ControlProjectedMachine, ControlProjectionMessage, ControlProjectionSnapshot,
    ControlSnapshotReason, MachineId, NotificationIntentAcknowledgement, NotificationIntentBatch,
    Os, ProjectionAcknowledgement, ProjectionBatch, ProjectionFreshness, ProjectionSyncAction,
    ProjectionSyncDirective, PushProviderMode, Secret, Timestamp, MAX_PROJECTION_BATCH_BYTES,
    MAX_PROJECTION_CHANGES, PROJECTION_SCHEMA_VERSION,
};
use secrets::{ControlSecrets, PresentedCredential};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use store::{
    registry::{ExchangeRecord, LinkRow, PairingCodeRow, RegistryRow, RegistryState},
    Store,
};

const CODE_LIFETIME_MS: i64 = 10 * 60 * 1_000;
const WINDOW_MS: i64 = 10 * 60 * 1_000;
const MAX_OUTSTANDING_CODES: u32 = 5;
const MAX_CODES_PER_WINDOW: u32 = 10;
const MAX_FAILURES_PER_WINDOW: u32 = 20;
const OFFLINE_AFTER_MS: i64 = 90_000;
const MAX_CLOCK_SKEW_MS: i64 = 5 * 60 * 1_000;
const CREDENTIAL_LIFETIME_MS: i64 = 90 * 24 * 60 * 60 * 1_000;
const REFRESH_WITHIN_MS: i64 = 30 * 24 * 60 * 60 * 1_000;
const REFRESH_GRACE_MS: i64 = 60_000;
const NOTIFICATION_MAX_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
const PROJECTION_LIVE_MS: i64 = 10_000;
const MAX_PROJECTED_MACHINES: usize = 100;
const MAX_PROJECTED_SESSIONS: u32 = 1_000;
const MAX_PROJECTED_EVENTS: u32 = 1_000;
const MAX_PROJECTED_METRICS: u32 = 1_001;
const MAX_PROJECTION_CATCH_UP: u32 = 1_000;

type Clock = Arc<dyn Fn() -> Timestamp + Send + Sync>;

/// Static settings of a control daemon.
#[derive(Debug, Clone)]
pub struct ControlSettings {
    /// Origin shown in the connect command.
    pub public_url: String,
    /// Heartbeat interval sent to workers.
    pub heartbeat_interval_ms: u64,
    /// Development mode also accepts loopback worker endpoints.
    pub development: bool,
    /// This control's own Machine ID.
    pub machine_id: MachineId,
    /// Display name used when the control first registers itself.
    pub display_name: String,
    /// Name discovery shows to Tailnet peers (`[control] display_name`);
    /// `None` uses `display_name`.
    pub discovery_name: Option<String>,
    /// Package directory (`[packages] directory`), for the stable release workers can install.
    pub package_dir: Option<PathBuf>,
    /// This control's OS.
    pub os: Os,
    /// This daemon's version.
    pub daemon_version: String,
    /// Owner-only secrets file.
    pub secrets_file: PathBuf,
    /// Directory for `pairing-audit.jsonl`.
    pub audit_dir: PathBuf,
    /// Whether the disposable dashboard projection is accepted and served.
    pub read_model_enabled: bool,
}

/// A refused control request: HTTP status plus a secret-free body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFailure {
    /// HTTP status to return.
    pub status: u16,
    /// Response body.
    pub failure: LinkFailure,
}

impl ControlFailure {
    pub(crate) fn new(status: u16, code: LinkFailureCode, message: impl Into<String>) -> Self {
        ControlFailure {
            status,
            failure: LinkFailure {
                code,
                message: message.into(),
            },
        }
    }

    fn internal(error: impl std::fmt::Display) -> Self {
        tracing::error!(%error, "control request failed");
        Self::new(
            500,
            LinkFailureCode::Internal,
            "the control could not complete the request",
        )
    }
}

/// Failure opening the control role.
#[derive(Debug, thiserror::Error)]
#[error("cannot start the control Machine role: {0}")]
pub struct ControlError(String);

/// Shared control-role state.
#[derive(Clone)]
pub struct ControlService {
    store: Arc<Store>,
    settings: Arc<ControlSettings>,
    clock: Clock,
    secrets: Arc<Mutex<ControlSecrets>>,
    audit: LinkAudit,
    projection_updates: tokio::sync::broadcast::Sender<ControlProjectionMessage>,
    refresh_fault: Arc<Mutex<Option<RefreshFault>>>,
    discovery_limit: Arc<discovery::DiscoveryLimit>,
    verifier: Arc<dyn PairingVerifier>,
    worker_updates: Arc<
        Mutex<std::collections::HashMap<MachineId, protocol::worker_update::WorkerUpdateStatus>>,
    >,
    worker_http: reqwest::Client,
}

/// One authenticated worker destination. The credential must never leave the
/// control daemon.
#[derive(Clone, Debug)]
pub struct RoutingTarget {
    /// Immutable worker identity expected from `/link/identity`.
    pub machine_id: MachineId,
    /// Approved address reported by the worker's latest heartbeat.
    pub endpoint: LinkEndpoint,
    /// Credential presented only by the control to this worker.
    pub control_credential: Secret,
    /// Capabilities reported by this worker.
    pub capabilities: Vec<MachineCapability>,
}

impl ControlService {
    /// Load secrets and register the control itself in the registry.
    pub fn open(
        store: Arc<Store>,
        settings: ControlSettings,
        clock: Clock,
    ) -> Result<Self, ControlError> {
        let secrets = ControlSecrets::load(&settings.secrets_file).map_err(ControlError)?;
        let now = clock();
        store
            .upsert_control_machine(&RegistryRow {
                machine_id: settings.machine_id.clone(),
                display_name: validate_display_name(&settings.display_name)
                    .unwrap_or_else(|_| "Control".into()),
                role: MachineRole::Control,
                os: Some(settings.os),
                daemon_version: Some(settings.daemon_version.clone()),
                capabilities: all_capabilities(),
                endpoint: None,
                state: RegistryState::Active,
                paired_at: now,
                last_seen: Some(now),
                last_sequence: 0,
            })
            .map_err(|error| ControlError(error.to_string()))?;
        let (projection_updates, _) = tokio::sync::broadcast::channel(256);
        let service = ControlService {
            store,
            audit: LinkAudit::new(&settings.audit_dir, "pairing-audit.jsonl"),
            settings: Arc::new(settings),
            clock,
            secrets: Arc::new(Mutex::new(secrets)),
            projection_updates,
            refresh_fault: Arc::new(Mutex::new(None)),
            discovery_limit: Arc::new(discovery::DiscoveryLimit::default()),
            verifier: Arc::new(TailnetVerifier::default()),
            worker_updates: Arc::default(),
            worker_http: worker_update::worker_client(),
        };
        // A refresh interrupted by a crash never reached the worker: undo it.
        service
            .reconcile_refresh()
            .map_err(|failure| ControlError(failure.failure.message))?;
        Ok(service)
    }

    /// This control's Machine ID.
    pub fn machine_id(&self) -> &MachineId {
        &self.settings.machine_id
    }

    /// Create a one-time code; the code is returned only here.
    pub fn create_code(&self) -> Result<PairingCode, ControlFailure> {
        let now = (self.clock)();
        self.audit_expired(now);
        let since = Timestamp(now.0 - WINDOW_MS);
        if self
            .store
            .count_outstanding_codes(now)
            .map_err(ControlFailure::internal)?
            >= MAX_OUTSTANDING_CODES
            || self
                .store
                .count_codes_created_since(since)
                .map_err(ControlFailure::internal)?
                >= MAX_CODES_PER_WINDOW
        {
            return Err(ControlFailure::new(
                429,
                LinkFailureCode::RateLimited,
                "too many pairing codes; cancel one or try again in a few minutes",
            ));
        }
        let code = link_crypto::new_pairing_code().map_err(ControlFailure::internal)?;
        let code_id = link_crypto::new_handle("pc").map_err(ControlFailure::internal)?;
        let expires_at = Timestamp(now.0 + CODE_LIFETIME_MS);
        self.store
            .insert_pairing_code(&PairingCodeRow {
                code_id: code_id.clone(),
                code_hmac: self.code_hmac(&code)?,
                created_at: now,
                expires_at,
                consumed_at: None,
                cancelled_at: None,
                machine_id: None,
            })
            .map_err(ControlFailure::internal)?;
        self.audit.append(&LinkAuditRecord {
            code_id: Some(&code_id),
            ..LinkAuditRecord::new(now, "CODE_CREATED")
        });
        Ok(PairingCode {
            code_id,
            code: Secret::new(link_crypto::display_code(&code)),
            expires_at,
            connect_command: connect_command(&self.settings.public_url),
        })
    }

    /// Current state of a code; never returns the code.
    pub fn code_status(&self, code_id: &str) -> Result<PairingCodeStatus, ControlFailure> {
        let now = (self.clock)();
        self.audit_expired(now);
        let row = self
            .store
            .pairing_code(code_id)
            .map_err(ControlFailure::internal)?
            .ok_or_else(|| {
                ControlFailure::new(404, LinkFailureCode::NotFound, "unknown pairing code")
            })?;
        let state = if row.consumed_at.is_some() {
            PairingCodeState::Connected
        } else if row.cancelled_at.is_some() {
            PairingCodeState::Cancelled
        } else if now.0 >= row.expires_at.0 {
            PairingCodeState::Expired
        } else {
            PairingCodeState::Waiting
        };
        Ok(PairingCodeStatus {
            code_id: row.code_id,
            state,
            expires_at: row.expires_at,
            machine_id: row.machine_id,
        })
    }

    /// Cancel one outstanding code.
    pub fn cancel_code(&self, code_id: &str) -> Result<(), ControlFailure> {
        let now = (self.clock)();
        if self
            .store
            .cancel_pairing_code(code_id, now)
            .map_err(ControlFailure::internal)?
        {
            self.audit.append(&LinkAuditRecord {
                code_id: Some(code_id),
                ..LinkAuditRecord::new(now, "CODE_CANCELLED")
            });
            Ok(())
        } else {
            Err(ControlFailure::new(
                404,
                LinkFailureCode::NotFound,
                "no outstanding pairing code with that ID",
            ))
        }
    }

    /// Cancel every outstanding code (password change, lockout).
    pub fn cancel_all_codes(&self, reason: &str) -> Result<usize, ControlFailure> {
        let now = (self.clock)();
        let cancelled = self
            .store
            .cancel_outstanding_codes(now)
            .map_err(ControlFailure::internal)?;
        if cancelled > 0 {
            self.audit.append(&LinkAuditRecord {
                reason: Some(reason),
                ..LinkAuditRecord::new(now, "CODE_CANCELLED")
            });
        }
        Ok(cancelled)
    }

    /// Exchange a one-time code for a new link.
    pub fn exchange(
        &self,
        request: LinkExchangeRequest,
        peer: &str,
    ) -> Result<LinkExchangeResponse, ControlFailure> {
        let now = (self.clock)();
        self.audit_expired(now);
        if self.locked(now)? {
            return Err(locked_failure());
        }
        let machine = self.validate_exchange(&request, peer, now)?;
        let Ok(normalized) = normalize_pairing_code(request.pairing_code.expose()) else {
            return Err(self.wrong_code(now, peer)?);
        };
        let worker_credential = link_crypto::new_credential().map_err(ControlFailure::internal)?;
        let link_id = link_crypto::new_handle("lk").map_err(ControlFailure::internal)?;
        let expires_at = Timestamp(now.0 + CREDENTIAL_LIFETIME_MS);
        // Secret first: a committed link must always have its credential.
        self.with_secrets(|secrets| {
            secrets.insert(
                &link_id,
                PresentedCredential {
                    machine_id: machine.machine_id.clone(),
                    control_credential: request.control_credential.clone(),
                },
            )
        })?;
        let outcome = self.store.complete_exchange(&ExchangeRecord {
            code_hmac: self.code_hmac(&normalized)?,
            now,
            link: LinkRow {
                link_id: link_id.clone(),
                machine_id: machine.machine_id.clone(),
                worker_credential_sha256: link_crypto::sha256_hex(&worker_credential),
                created_at: now,
                expires_at,
                previous_sha256: None,
                previous_valid_until: None,
                revoked_at: None,
                last_used_at: None,
            },
            machine,
        });
        let outcome = match outcome {
            Ok(Some(outcome)) => outcome,
            Ok(None) => {
                self.with_secrets(|secrets| secrets.remove(std::slice::from_ref(&link_id)))?;
                return Err(self.wrong_code(now, peer)?);
            }
            Err(error) => {
                let _ = self.with_secrets(|secrets| secrets.remove(std::slice::from_ref(&link_id)));
                return Err(ControlFailure::internal(error));
            }
        };
        if !outcome.replaced_links.is_empty() {
            self.with_secrets(|secrets| secrets.remove(&outcome.replaced_links))?;
            self.audit.append(&LinkAuditRecord {
                machine_id: Some(&request.machine.machine_id.0),
                link_id: Some(&link_id),
                ..LinkAuditRecord::new(now, "LINK_REPLACED")
            });
        }
        self.audit.append(&LinkAuditRecord {
            code_id: Some(&outcome.code_id),
            machine_id: Some(&request.machine.machine_id.0),
            link_id: Some(&link_id),
            peer: Some(peer),
            ..LinkAuditRecord::new(now, "EXCHANGE_SUCCEEDED")
        });
        Ok(LinkExchangeResponse {
            link_id,
            control_machine_id: self.settings.machine_id.clone(),
            worker_credential: Secret::new(worker_credential),
            credential_expires_at: expires_at,
            display_name: outcome.display_name,
            heartbeat_interval_ms: self.settings.heartbeat_interval_ms,
        })
    }

    /// Accept a worker heartbeat.
    pub fn heartbeat(
        &self,
        bearer: &str,
        request: LinkHeartbeatRequest,
    ) -> Result<LinkHeartbeatResponse, ControlFailure> {
        let now = (self.clock)();
        let link = self.authenticate(bearer, &request.machine_id, now)?;
        if (request.sent_at.0 - now.0).abs() > MAX_CLOCK_SKEW_MS {
            return Err(ControlFailure::new(
                400,
                LinkFailureCode::InvalidRequest,
                "heartbeat time differs from the control clock by more than 5 minutes",
            ));
        }
        self.validate_endpoint(&request.endpoint)
            .map_err(|message| {
                ControlFailure::new(400, LinkFailureCode::InvalidRequest, message)
            })?;
        let previous = self
            .store
            .registry_machine(&request.machine_id)
            .map_err(ControlFailure::internal)?
            .and_then(|row| row.endpoint);
        let accepted = self
            .store
            .record_heartbeat(
                &request.machine_id,
                &link.link_id,
                request.sequence,
                now,
                &request.endpoint,
                &request.daemon_version,
                &request.capabilities,
            )
            .map_err(ControlFailure::internal)?;
        if !accepted {
            self.audit.machine(
                now,
                "HEARTBEAT_REJECTED",
                &request.machine_id,
                Some("STALE_SEQUENCE"),
            );
            return Err(ControlFailure::new(
                409,
                LinkFailureCode::StaleSequence,
                format!(
                    "heartbeat sequence {} is not newer than the last one",
                    request.sequence
                ),
            ));
        }
        if previous.is_some_and(|previous| previous != request.endpoint) {
            let change = format!("{}:{}", request.endpoint.address, request.endpoint.port);
            self.audit
                .machine(now, "ENDPOINT_CHANGED", &request.machine_id, Some(&change));
        }
        let display_name = self
            .store
            .registry_machine(&request.machine_id)
            .map_err(ControlFailure::internal)?
            .map(|row| row.display_name)
            .unwrap_or_default();
        let projection_sync = request
            .projection_sync
            .as_ref()
            .map(|report| self.projection_directive(&request.machine_id, report))
            .transpose()?;
        self.record_worker_update(&request.machine_id, request.update_status.clone());
        Ok(LinkHeartbeatResponse {
            available_release: self.available_release(),
            status: LinkStatus::Active,
            display_name,
            next_heartbeat_ms: self.settings.heartbeat_interval_ms,
            // A worker still on its previous credential missed a refresh reply.
            refresh_due: link.expires_at.0 - now.0 < REFRESH_WITHIN_MS
                || link.worker_credential_sha256 != link_crypto::sha256_hex(bearer),
            notification_ingress: Some(
                self.store.push_provider_mode().ok() == Some(PushProviderMode::WebPush),
            ),
            projection_sync,
        })
    }

    /// Authenticate and durably apply one bounded projection upload.
    pub fn accept_projection(
        &self,
        bearer: &str,
        batch: ProjectionBatch,
    ) -> Result<ProjectionAcknowledgement, ControlFailure> {
        self.ensure_read_model_enabled()?;
        let now = (self.clock)();
        let machine_id = match &batch {
            ProjectionBatch::Delta { machine_id, .. }
            | ProjectionBatch::Snapshot { machine_id, .. } => machine_id,
        }
        .clone();
        self.authenticate(bearer, &machine_id, now)?;
        self.commit_projection(&machine_id, batch, now, true)
    }

    /// Apply this control Machine's own local projection without loopback HTTP.
    pub fn accept_local_projection(
        &self,
        batch: ProjectionBatch,
    ) -> Result<ProjectionAcknowledgement, ControlFailure> {
        let machine_id = match &batch {
            ProjectionBatch::Delta { machine_id, .. }
            | ProjectionBatch::Snapshot { machine_id, .. } => machine_id,
        }
        .clone();
        if machine_id != self.settings.machine_id {
            return Err(ControlFailure::new(
                400,
                LinkFailureCode::InvalidMachine,
                "local projection must belong to the control Machine",
            ));
        }
        self.commit_projection(&machine_id, batch, (self.clock)(), false)
    }

    fn commit_projection(
        &self,
        machine_id: &MachineId,
        batch: ProjectionBatch,
        now: Timestamp,
        audit: bool,
    ) -> Result<ProjectionAcknowledgement, ControlFailure> {
        let before_revision = self
            .store
            .control_projection_revision()
            .map_err(ControlFailure::internal)?;
        let changes = match &batch {
            ProjectionBatch::Delta { changes, .. } => Some(changes.clone()),
            ProjectionBatch::Snapshot { .. } => None,
        };
        let acknowledgement = self
            .store
            .apply_projection_batch(machine_id, &batch, now)
            .map_err(ControlFailure::internal)?;
        if acknowledgement.status == protocol::ProjectionAcknowledgementStatus::Accepted {
            let after_revision = self
                .store
                .control_projection_revision()
                .map_err(ControlFailure::internal)?;
            if after_revision > before_revision {
                let update = changes.map_or(
                    ControlProjectionMessage::SnapshotRequired {
                        current_revision: after_revision,
                        reason: ControlSnapshotReason::RevisionMismatch,
                    },
                    |changes| ControlProjectionMessage::ProjectionDelta {
                        machine_id: Some(machine_id.clone()),
                        from_revision: before_revision,
                        to_revision: after_revision,
                        changes,
                    },
                );
                let _ = self.projection_updates.send(update);
            }
        }
        let metadata = format!(
            "status={:?},cursor={},bytes={}",
            acknowledgement.status,
            acknowledgement.committed_cursor,
            serde_json::to_vec(&batch).map_or(0, |bytes| bytes.len())
        );
        if audit {
            self.audit
                .machine(now, "PROJECTION_BATCH", machine_id, Some(&metadata));
        }
        Ok(acknowledgement)
    }

    /// Subscribe to committed projection changes for the owner live stream.
    pub fn subscribe_projection_updates(
        &self,
    ) -> tokio::sync::broadcast::Receiver<ControlProjectionMessage> {
        self.projection_updates.subscribe()
    }

    /// Return a bounded revision-ordered catch-up for a reconnecting owner stream.
    pub fn projection_updates_after(
        &self,
        after: u64,
    ) -> Result<Vec<ControlProjectionMessage>, ControlFailure> {
        self.ensure_read_model_enabled()?;
        let current = self
            .store
            .control_projection_revision()
            .map_err(ControlFailure::internal)?;
        if after > current {
            return Ok(vec![ControlProjectionMessage::SnapshotRequired {
                current_revision: current,
                reason: ControlSnapshotReason::RevisionMismatch,
            }]);
        }
        let revisions = self
            .store
            .control_projection_changes_after(after, MAX_PROJECTION_CATCH_UP + 1)
            .map_err(ControlFailure::internal)?;
        if revisions.len() > MAX_PROJECTION_CATCH_UP as usize
            || revisions
                .first()
                .is_some_and(|revision| revision.revision != after.saturating_add(1))
        {
            return Ok(vec![ControlProjectionMessage::SnapshotRequired {
                current_revision: current,
                reason: ControlSnapshotReason::DeltaBoundExceeded,
            }]);
        }
        Ok(revisions
            .into_iter()
            .map(|revision| ControlProjectionMessage::ProjectionDelta {
                machine_id: Some(revision.machine_id),
                from_revision: revision.revision.saturating_sub(1),
                to_revision: revision.revision,
                changes: vec![revision.change],
            })
            .collect())
    }

    /// Build one bounded owner dashboard snapshot from local SQLite only.
    pub fn projection_snapshot(&self) -> Result<ControlProjectionSnapshot, ControlFailure> {
        self.ensure_read_model_enabled()?;
        let now = (self.clock)();
        let registry = self.registry()?;
        let omitted_machines = registry
            .machines
            .len()
            .saturating_sub(MAX_PROJECTED_MACHINES) as u32;
        let mut omitted_sessions = 0_u32;
        let mut remaining_sessions = MAX_PROJECTED_SESSIONS;
        let mut remaining_events = MAX_PROJECTED_EVENTS;
        let mut remaining_metrics = MAX_PROJECTED_METRICS;
        let mut machines = Vec::new();
        for machine in registry.machines.into_iter().take(MAX_PROJECTED_MACHINES) {
            let state = self
                .store
                .control_projection_state(&machine.machine_id)
                .map_err(ControlFailure::internal)?;
            let session_count = self
                .store
                .projected_session_count(&machine.machine_id)
                .map_err(ControlFailure::internal)?;
            let sessions = self
                .store
                .projected_sessions(&machine.machine_id, remaining_sessions)
                .map_err(ControlFailure::internal)?;
            let returned_sessions = sessions.len() as u32;
            remaining_sessions = remaining_sessions.saturating_sub(returned_sessions);
            let machine_omitted = session_count.saturating_sub(returned_sessions);
            omitted_sessions = omitted_sessions.saturating_add(machine_omitted);
            let credential_expired = machine
                .credential
                .as_ref()
                .is_some_and(|credential| credential.expires_at.0 <= now.0);
            let freshness =
                projection_freshness(machine.status, credential_expired, state.as_ref(), now);
            machines.push(ControlProjectedMachine {
                machine_id: machine.machine_id.clone(),
                projection_available: state.is_some(),
                display_name: machine.display_name,
                os: machine.os,
                daemon_version: machine.daemon_version,
                capabilities: machine.capabilities,
                registry_status: machine.status,
                freshness,
                last_seen: machine.last_seen,
                observed_at: state.as_ref().and_then(|value| value.observed_at),
                synced_at: state.as_ref().map(|value| value.synced_at),
                source_cursor: state.as_ref().map_or(0, |value| value.committed_cursor),
                sessions,
                events: {
                    let values = self
                        .store
                        .projected_events(&machine.machine_id, remaining_events)
                        .map_err(ControlFailure::internal)?;
                    remaining_events = remaining_events.saturating_sub(values.len() as u32);
                    values
                },
                metrics: {
                    let values = self
                        .store
                        .projected_metrics(&machine.machine_id, remaining_metrics)
                        .map_err(ControlFailure::internal)?;
                    remaining_metrics = remaining_metrics.saturating_sub(values.len() as u32);
                    values
                },
                omitted_sessions: machine_omitted,
            });
        }
        Ok(ControlProjectionSnapshot {
            schema_version: PROJECTION_SCHEMA_VERSION,
            control_machine_id: Some(self.settings.machine_id.clone()),
            view_revision: self
                .store
                .control_projection_revision()
                .map_err(ControlFailure::internal)?,
            generated_at: now,
            machines,
            omitted_machines,
            omitted_sessions,
            truncated: omitted_machines > 0 || omitted_sessions > 0,
        })
    }

    fn projection_directive(
        &self,
        machine_id: &MachineId,
        report: &protocol::ProjectionSyncReport,
    ) -> Result<ProjectionSyncDirective, ControlFailure> {
        if !self.settings.read_model_enabled {
            return Ok(ProjectionSyncDirective {
                enabled: false,
                schema_version: PROJECTION_SCHEMA_VERSION,
                committed_cursor: 0,
                max_batch_bytes: MAX_PROJECTION_BATCH_BYTES as u32,
                max_changes: MAX_PROJECTION_CHANGES as u16,
                action: ProjectionSyncAction::Disabled,
            });
        }
        let stored = self
            .store
            .control_projection_state(machine_id)
            .map_err(ControlFailure::internal)?;
        let committed = stored
            .as_ref()
            .filter(|state| state.source_epoch == report.source_epoch)
            .map_or(0, |state| state.committed_cursor);
        let action = if report.snapshot_required
            || report.schema_version != PROJECTION_SCHEMA_VERSION
            || stored
                .as_ref()
                .is_some_and(|state| state.source_epoch != report.source_epoch)
            || report.current_cursor < committed
        {
            ProjectionSyncAction::Snapshot
        } else if report.current_cursor > committed {
            ProjectionSyncAction::Upload
        } else {
            ProjectionSyncAction::Wait
        };
        Ok(ProjectionSyncDirective {
            enabled: true,
            schema_version: PROJECTION_SCHEMA_VERSION,
            committed_cursor: committed,
            max_batch_bytes: MAX_PROJECTION_BATCH_BYTES as u32,
            max_changes: MAX_PROJECTION_CHANGES as u16,
            action,
        })
    }

    fn ensure_read_model_enabled(&self) -> Result<(), ControlFailure> {
        if self.settings.read_model_enabled {
            Ok(())
        } else {
            Err(ControlFailure::new(
                404,
                LinkFailureCode::NotFound,
                "control read model is disabled",
            ))
        }
    }

    /// Authenticate and durably accept one bounded worker notification batch.
    pub fn accept_notification_intents(
        &self,
        bearer: &str,
        batch: NotificationIntentBatch,
    ) -> Result<NotificationIntentAcknowledgement, ControlFailure> {
        let now = (self.clock)();
        self.authenticate(bearer, &batch.machine_id, now)?;
        if self
            .store
            .push_provider_mode()
            .map_err(ControlFailure::internal)?
            != PushProviderMode::WebPush
        {
            return Err(ControlFailure::new(
                409,
                LinkFailureCode::InvalidRequest,
                "native notification ingress is disabled on this control",
            ));
        }
        let acknowledgement = self
            .store
            .accept_notification_intents(&batch, now, NOTIFICATION_MAX_AGE_MS, MAX_CLOCK_SKEW_MS)
            .map_err(ControlFailure::internal)?;
        let metadata = format!(
            "accepted={},duplicates={},rejected={},bytes={}",
            acknowledgement.accepted.len(),
            acknowledgement.duplicates.len(),
            acknowledgement.rejected.len(),
            serde_json::to_vec(&batch).map_or(0, |bytes| bytes.len())
        );
        self.audit.machine(
            now,
            "NOTIFICATION_INTENTS",
            &batch.machine_id,
            Some(&metadata),
        );
        Ok(acknowledgement)
    }

    /// A worker ends its own link.
    pub fn disconnect(&self, bearer: &str, machine_id: &MachineId) -> Result<(), ControlFailure> {
        let now = (self.clock)();
        self.authenticate(bearer, machine_id, now)?;
        let links = self
            .store
            .disconnect_registry_machine(machine_id, now)
            .map_err(ControlFailure::internal)?
            .unwrap_or_default();
        self.with_secrets(|secrets| secrets.remove(&links))?;
        self.audit.machine(now, "DISCONNECTED", machine_id, None);
        self.notify_registry_changed();
        Ok(())
    }

    /// Tell every subscribed owner live stream to refetch a fresh snapshot:
    /// registry state (disconnect/revoke/rename) isn't a projection change
    /// with its own delta, so there is nothing to carry but the signal.
    fn notify_registry_changed(&self) {
        let current_revision = self.store.control_projection_revision().unwrap_or(0);
        let _ = self
            .projection_updates
            .send(ControlProjectionMessage::SnapshotRequired {
                current_revision,
                reason: ControlSnapshotReason::RegistryChanged,
            });
    }

    /// The owner-visible registry; never contains a secret.
    pub fn registry(&self) -> Result<RegistryMachines, ControlFailure> {
        let now = (self.clock)();
        let rows = self
            .store
            .registry_machines()
            .map_err(ControlFailure::internal)?;
        let machines = rows
            .into_iter()
            .map(|row| self.registry_entry(row, now))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(RegistryMachines {
            machines,
            available_release: self.available_release(),
        })
    }

    /// Rename a Machine (owner only); the ID never changes.
    pub fn rename(
        &self,
        machine_id: &MachineId,
        display_name: &str,
    ) -> Result<RegistryMachine, ControlFailure> {
        let now = (self.clock)();
        let name = validate_display_name(display_name).map_err(|error| {
            ControlFailure::new(400, LinkFailureCode::InvalidRequest, error.to_string())
        })?;
        if !self
            .store
            .rename_registry_machine(machine_id, &name)
            .map_err(ControlFailure::internal)?
        {
            return Err(not_found());
        }
        self.audit.machine(now, "RENAMED", machine_id, None);
        self.notify_registry_changed();
        let row = self
            .store
            .registry_machine(machine_id)
            .map_err(ControlFailure::internal)?
            .ok_or_else(not_found)?;
        self.registry_entry(row, now)
    }

    /// Revoke one worker; other Machines and browser Sessions are untouched.
    pub fn revoke(&self, machine_id: &MachineId) -> Result<(), ControlFailure> {
        let now = (self.clock)();
        if machine_id == &self.settings.machine_id {
            return Err(ControlFailure::new(
                400,
                LinkFailureCode::InvalidRequest,
                "the control Machine cannot be revoked",
            ));
        }
        let links = self
            .store
            .revoke_registry_machine(machine_id, now)
            .map_err(ControlFailure::internal)?
            .ok_or_else(not_found)?;
        self.store
            .purge_control_projection(machine_id)
            .map_err(ControlFailure::internal)?;
        self.with_secrets(|secrets| secrets.remove(&links))?;
        self.audit.machine(now, "REVOKED", machine_id, None);
        self.notify_registry_changed();
        Ok(())
    }

    /// The credential the control presents to a linked worker (for 4.A routing).
    pub fn presented_credential(&self, machine_id: &MachineId) -> Option<Secret> {
        let link = self.store.active_link_for(machine_id).ok().flatten()?;
        let secrets = self.secrets.lock().ok()?;
        secrets
            .credential(&link.link_id)
            .filter(|credential| &credential.machine_id == machine_id)
            .map(|credential| credential.control_credential.clone())
    }

    /// Resolve one active worker to its endpoint and private control credential.
    pub fn routing_target(&self, machine_id: &MachineId) -> Result<RoutingTarget, ControlFailure> {
        let row = self
            .store
            .registry_machine(machine_id)
            .map_err(ControlFailure::internal)?
            .ok_or_else(not_found)?;
        if row.role != MachineRole::Worker {
            return Err(ControlFailure::new(
                400,
                LinkFailureCode::InvalidRequest,
                "the control Machine is served locally",
            ));
        }
        if row.state == RegistryState::Revoked {
            return Err(ControlFailure::new(
                410,
                LinkFailureCode::LinkRevoked,
                "this Machine link was revoked",
            ));
        }
        if row.state != RegistryState::Active {
            return Err(ControlFailure::new(
                409,
                LinkFailureCode::InvalidRequest,
                "this Machine is not connected",
            ));
        }
        let endpoint = row.endpoint.ok_or_else(|| {
            ControlFailure::new(
                503,
                LinkFailureCode::Internal,
                "this Machine has no approved endpoint",
            )
        })?;
        let link = self
            .store
            .active_link_for(machine_id)
            .map_err(ControlFailure::internal)?
            .ok_or_else(|| {
                ControlFailure::new(
                    503,
                    LinkFailureCode::Internal,
                    "this Machine has no active link",
                )
            })?;
        if link.expires_at.0 <= (self.clock)().0 {
            return Err(ControlFailure::new(
                401,
                LinkFailureCode::Unauthorized,
                "this Machine credential expired",
            ));
        }
        let control_credential = self
            .secrets
            .lock()
            .ok()
            .and_then(|secrets| secrets.credential(&link.link_id).cloned())
            .filter(|credential| &credential.machine_id == machine_id)
            .map(|credential| credential.control_credential)
            .ok_or_else(|| {
                ControlFailure::new(
                    503,
                    LinkFailureCode::Internal,
                    "this Machine has no active control credential",
                )
            })?;
        Ok(RoutingTarget {
            machine_id: machine_id.clone(),
            endpoint,
            control_credential,
            capabilities: row.capabilities,
        })
    }

    /// Record an origin rejection seen by the HTTP layer.
    pub fn audit_origin_rejected(&self, reason: &str) {
        self.audit.append(&LinkAuditRecord {
            reason: Some(reason),
            ..LinkAuditRecord::new((self.clock)(), "ORIGIN_REJECTED")
        });
    }

    /// Audit codes that expired unused since the last check (once each).
    fn audit_expired(&self, now: Timestamp) {
        match self.store.take_newly_expired_codes(now) {
            Ok(codes) => {
                for code_id in codes {
                    self.audit.append(&LinkAuditRecord {
                        code_id: Some(&code_id),
                        ..LinkAuditRecord::new(now, "CODE_EXPIRED")
                    });
                }
            }
            Err(error) => tracing::warn!(%error, "failed to check expired pairing codes"),
        }
    }

    fn registry_entry(
        &self,
        row: RegistryRow,
        now: Timestamp,
    ) -> Result<RegistryMachine, ControlFailure> {
        let status = match (row.role, row.state) {
            (MachineRole::Control, _) => RegistryStatus::Online,
            (_, RegistryState::Revoked) => RegistryStatus::Revoked,
            (_, RegistryState::Disconnected) => RegistryStatus::Disconnected,
            (_, RegistryState::Active) => match row.last_seen {
                Some(seen) if now.0 - seen.0 < OFFLINE_AFTER_MS => RegistryStatus::Online,
                _ => RegistryStatus::Offline,
            },
        };
        let credential = if row.role == MachineRole::Worker && row.state == RegistryState::Active {
            self.store
                .active_link_for(&row.machine_id)
                .map_err(ControlFailure::internal)?
                .map(|link| LinkCredentialInfo {
                    link_id: link.link_id,
                    created_at: link.created_at,
                    expires_at: link.expires_at,
                    last_used_at: link.last_used_at,
                })
        } else {
            None
        };
        let update = self.worker_update_view(&row.machine_id, now);
        Ok(RegistryMachine {
            machine_id: row.machine_id,
            display_name: row.display_name,
            role: row.role,
            status,
            last_seen: row.last_seen,
            endpoint: row.endpoint.filter(|_| row.state == RegistryState::Active),
            os: row.os,
            daemon_version: row.daemon_version,
            capabilities: row.capabilities,
            credential,
            update,
        })
    }

    /// Resolve a worker bearer to its link, distinguishing revoked links.
    fn authenticate(
        &self,
        bearer: &str,
        machine_id: &MachineId,
        now: Timestamp,
    ) -> Result<LinkRow, ControlFailure> {
        let unauthorized = || {
            ControlFailure::new(
                401,
                LinkFailureCode::Unauthorized,
                "missing or invalid worker credential",
            )
        };
        if !link_crypto::is_credential(bearer) {
            return Err(unauthorized());
        }
        let hash = link_crypto::sha256_hex(bearer);
        match self
            .store
            .active_link_by_hash(&hash, now)
            .map_err(ControlFailure::internal)?
        {
            Some(link) if &link.machine_id == machine_id => Ok(link),
            Some(_) => Err(unauthorized()),
            None => {
                let revoked = self
                    .store
                    .link_by_hash(&hash)
                    .map_err(ControlFailure::internal)?
                    .is_some_and(|link| {
                        link.revoked_at.is_some() && &link.machine_id == machine_id
                    });
                if revoked {
                    self.audit
                        .machine(now, "HEARTBEAT_REJECTED", machine_id, Some("LINK_REVOKED"));
                    Err(ControlFailure::new(
                        401,
                        LinkFailureCode::LinkRevoked,
                        "this Machine link was revoked",
                    ))
                } else {
                    Err(unauthorized())
                }
            }
        }
    }

    fn validate_exchange(
        &self,
        request: &LinkExchangeRequest,
        peer: &str,
        now: Timestamp,
    ) -> Result<RegistryRow, ControlFailure> {
        let invalid = |reason: String| {
            self.audit.append(&LinkAuditRecord {
                peer: Some(peer),
                reason: Some("INVALID_MACHINE"),
                ..LinkAuditRecord::new(now, "EXCHANGE_FAILED")
            });
            ControlFailure::new(400, LinkFailureCode::InvalidMachine, reason)
        };
        let machine = &request.machine;
        if !machine.machine_id.is_ulid() {
            return Err(invalid("machine_id must be a ULID".into()));
        }
        if machine.machine_id == self.settings.machine_id {
            return Err(invalid(
                "a worker cannot use the control's Machine ID".into(),
            ));
        }
        let display_name = validate_display_name(&machine.display_name)
            .map_err(|error| invalid(error.to_string()))?;
        if machine.daemon_version.is_empty() || machine.daemon_version.len() > 64 {
            return Err(invalid("daemon_version must be 1 to 64 bytes".into()));
        }
        self.validate_endpoint(&request.endpoint)
            .map_err(|message| invalid(message.into()))?;
        if !link_crypto::is_credential(request.control_credential.expose()) {
            return Err(invalid(
                "control_credential must be 64 hex characters".into(),
            ));
        }
        Ok(RegistryRow {
            machine_id: machine.machine_id.clone(),
            display_name,
            role: MachineRole::Worker,
            os: Some(machine.os),
            daemon_version: Some(machine.daemon_version.clone()),
            capabilities: machine.capabilities.clone(),
            endpoint: Some(request.endpoint.clone()),
            state: RegistryState::Active,
            paired_at: now,
            last_seen: None,
            last_sequence: 0,
        })
    }

    /// Endpoints must be Tailscale addresses (loopback too, in development).
    fn validate_endpoint(&self, endpoint: &LinkEndpoint) -> Result<(), &'static str> {
        let Ok(address) = endpoint.address.parse::<std::net::IpAddr>() else {
            return Err("endpoint address must be an IP address");
        };
        let tailscale = match address {
            std::net::IpAddr::V4(ip) => {
                ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1])
            }
            std::net::IpAddr::V6(ip) => ip.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
        };
        if endpoint.port == 0 {
            return Err("endpoint port must not be 0");
        }
        if tailscale || (self.settings.development && address.is_loopback()) {
            Ok(())
        } else {
            Err("endpoint must be a Tailscale address")
        }
    }

    fn locked(&self, now: Timestamp) -> Result<bool, ControlFailure> {
        Ok(self
            .store
            .count_pairing_failures_since(Timestamp(now.0 - WINDOW_MS))
            .map_err(ControlFailure::internal)?
            >= MAX_FAILURES_PER_WINDOW)
    }

    /// Record a wrong code; the failure that reaches the limit locks pairing.
    fn wrong_code(&self, now: Timestamp, peer: &str) -> Result<ControlFailure, ControlFailure> {
        self.store
            .record_pairing_failure(now)
            .map_err(ControlFailure::internal)?;
        let locked = self.locked(now)?;
        self.audit.append(&LinkAuditRecord {
            peer: Some(peer),
            reason: Some(if locked { "LOCKED" } else { "INVALID_CODE" }),
            ..LinkAuditRecord::new(now, "EXCHANGE_FAILED")
        });
        if locked {
            self.cancel_all_codes("PAIRING_LOCKED")?;
            return Ok(locked_failure());
        }
        Ok(ControlFailure::new(
            401,
            LinkFailureCode::InvalidCode,
            "that pairing code is not valid",
        ))
    }

    fn code_hmac(&self, normalized: &str) -> Result<String, ControlFailure> {
        let key = self
            .secrets
            .lock()
            .map_err(|_| ControlFailure::internal("control secrets lock poisoned"))?
            .pairing_key();
        Ok(link_crypto::hmac_hex(&key, normalized))
    }

    fn with_secrets(
        &self,
        change: impl FnOnce(&mut ControlSecrets) -> Result<(), String>,
    ) -> Result<(), ControlFailure> {
        let mut secrets = self
            .secrets
            .lock()
            .map_err(|_| ControlFailure::internal("control secrets lock poisoned"))?;
        change(&mut secrets).map_err(ControlFailure::internal)
    }
}

fn projection_freshness(
    registry: RegistryStatus,
    credential_expired: bool,
    state: Option<&store::ControlProjectionState>,
    now: Timestamp,
) -> ProjectionFreshness {
    if credential_expired {
        return ProjectionFreshness::AuthExpired;
    }
    match registry {
        RegistryStatus::Revoked => ProjectionFreshness::Revoked,
        RegistryStatus::Disconnected => ProjectionFreshness::Disconnected,
        RegistryStatus::Offline => ProjectionFreshness::Offline,
        RegistryStatus::Online => match state {
            Some(state) if now.0.saturating_sub(state.synced_at.0) <= PROJECTION_LIVE_MS => {
                ProjectionFreshness::Live
            }
            _ => ProjectionFreshness::Stale,
        },
    }
}

fn locked_failure() -> ControlFailure {
    ControlFailure::new(
        429,
        LinkFailureCode::PairingLocked,
        "pairing is locked for 10 minutes after too many wrong codes",
    )
}

fn not_found() -> ControlFailure {
    ControlFailure::new(404, LinkFailureCode::NotFound, "unknown Machine")
}

/// Every capability this daemon version serves.
pub fn all_capabilities() -> Vec<MachineCapability> {
    vec![
        MachineCapability::Sessions,
        MachineCapability::Events,
        MachineCapability::Metrics,
        MachineCapability::Adopt,
    ]
}
