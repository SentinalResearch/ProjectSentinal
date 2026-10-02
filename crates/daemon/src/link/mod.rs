//! Worker side of a Machine link: identity, pairing, heartbeats, and revocation.
//!
//! The worker always dials out to its control. It keeps the plaintext worker
//! credential it presents and only a SHA-256 of the control credential it
//! verifies. The pairing code passes through once, in one request body.

mod client;
mod file;

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
    LocalErrorCode, LocalRequest, LocalResponse, MachineId, Os, Secret, Timestamp,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

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
            }),
        };
        if matches!(manager.read_link(), Ok(Some(link)) if link.state == StoredState::Active) {
            manager.spawn_heartbeats();
        }
        manager
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
        let request = LinkHeartbeatRequest {
            machine_id: machine_id.clone(),
            sequence: link.sequence,
            sent_at: now,
            endpoint: self.inner.settings.endpoint.clone(),
            daemon_version: self.inner.settings.daemon_version.clone(),
            capabilities: all_capabilities(),
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
                link.display_name = Some(response.display_name);
                link.last_heartbeat_at = Some(now);
                link.heartbeat_interval_ms = response.next_heartbeat_ms;
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
                Err(LinkFailure::new(LocalErrorCode::ControlUnreachable, detail))
            }
        }
    }

    /// Identity proof for `GET /api/v1/link/identity`, given the control's bearer.
    pub fn verify_control_credential(&self, bearer: &str) -> Option<LinkIdentity> {
        let link = self.read_link().ok().flatten()?;
        let expected = link.control_credential_sha256.as_deref()?;
        if link.state != StoredState::Active
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
