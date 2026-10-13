//! The worker's side of approval pairing (4.E §6). The daemon creates the
//! 256-bit request secret, sends only its hash, answers the control's proof
//! callback for the one request it has open, and saves the link when the
//! claim succeeds. Nothing secret reaches the CLI; the request lives in
//! memory, so a restart abandons it (it expires on the control).

use super::{
    client::CallError, file::LinkFile, identity_failure, internal, LinkFailure, LinkManager,
};
use crate::link_crypto::{self, sha256_hex};
use protocol::{
    link::{
        validate_control_url, validate_display_name, ControlUrlPolicy, LinkFailureCode,
        LinkMachineInfo, Secret,
    },
    pairing::{
        is_request_id, verification_transcript, PairingCancel, PairingClaim, PairingClaimResult,
        PairingProof, PairingRequestCreate, PairingRequestCreated, PairingRequestState,
    },
    LocalErrorCode, LocalResponse, MachineId,
};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;

/// The one open request.
pub(super) struct PendingApproval {
    control_url: String,
    control_machine_id: MachineId,
    secret: String,
    secret_sha256: String,
    control_credential: String,
    /// Set by the first proof callback, then checked against the reply.
    request_id: Option<String>,
}

impl LinkManager {
    /// Create, and let the control verify, an approval request.
    pub(super) async fn request_approval(
        &self,
        control_url: &str,
        control_machine_id: MachineId,
        display_name: Option<&str>,
    ) -> Result<LocalResponse, LinkFailure> {
        let _operation = self.inner.operation.lock().await;
        let machine_id = self.machine_id().map_err(identity_failure)?;
        if let Some(link) = self.read_link()? {
            if link.state == super::file::StoredState::Active {
                return Err(LinkFailure::new(
                    LocalErrorCode::AlreadyConnected,
                    format!(
                        "this Machine is already connected to {}; run `sentinal disconnect` first",
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
        let display_name = validate_display_name(
            display_name.unwrap_or(&self.inner.settings.hostname),
        )
        .map_err(|error| LinkFailure::new(LocalErrorCode::InvalidRequest, error.to_string()))?;
        let random = || link_crypto::new_credential().map_err(internal);
        let secret = random()?;
        let secret_sha256 = sha256_hex(&secret);
        let worker_nonce = random()?[..32].to_string();
        self.set_pending(Some(PendingApproval {
            control_url: control_url.clone(),
            control_machine_id: control_machine_id.clone(),
            secret,
            secret_sha256: secret_sha256.clone(),
            control_credential: random()?,
            request_id: None,
        }))?;
        let request = PairingRequestCreate {
            machine: LinkMachineInfo {
                machine_id,
                display_name,
                os: self.inner.settings.os,
                daemon_version: self.inner.settings.daemon_version.clone(),
                capabilities: crate::control::all_capabilities(),
            },
            endpoint: self.inner.settings.endpoint.clone(),
            request_secret_sha256: secret_sha256.clone(),
            worker_nonce: worker_nonce.clone(),
        };
        let created: Result<Option<PairingRequestCreated>, CallError> = self
            .inner
            .client
            .post(&control_url, "/api/v1/pairing/requests", None, &request)
            .await;
        let created = match created {
            Ok(Some(created)) => created,
            Ok(None) => return Err(self.abandon(LocalErrorCode::ControlUnreachable, "empty reply")),
            Err(error) => {
                let (code, message) = match error.code() {
                    Some(LinkFailureCode::MachineAlreadyLinked) => (
                        LocalErrorCode::AlreadyConnected,
                        "the control already has an active link for this Machine; revoke it there or run `sentinal disconnect`".to_string(),
                    ),
                    Some(LinkFailureCode::PairingBusy | LinkFailureCode::PairingLocked) => (
                        LocalErrorCode::CodeRejected,
                        "the control has too many pairing requests waiting; try again in a few minutes".into(),
                    ),
                    Some(LinkFailureCode::InvalidMachine) => (
                        LocalErrorCode::CodeRejected,
                        "the control could not verify this Machine: either it cannot reach sentinald on its Tailscale address, or it is not the control that was discovered".into(),
                    ),
                    Some(LinkFailureCode::NotControl) | None => (
                        LocalErrorCode::ControlUnreachable,
                        format!("cannot pair with {control_url}: {error}"),
                    ),
                    Some(_) => (LocalErrorCode::InvalidRequest, error.to_string()),
                };
                return Err(self.abandon(code, &message));
            }
        };
        let bound = self
            .with_pending(|pending| pending.request_id.clone())?
            .flatten();
        if created.control_machine_id != control_machine_id {
            return Err(self.abandon(
                LocalErrorCode::ControlUnreachable,
                "this is a different control than the one discovered; pair again",
            ));
        }
        if !is_request_id(&created.request_id)
            || bound.as_deref() != Some(created.request_id.as_str())
        {
            return Err(self.abandon(
                LocalErrorCode::ControlUnreachable,
                "the control's reply does not match the request it verified",
            ));
        }
        let transcript = verification_transcript(
            &control_machine_id,
            &created.request_id,
            &created.control_nonce,
            &secret_sha256,
            &worker_nonce,
        );
        self.audit_link((self.inner.clock)(), "APPROVAL_REQUESTED", None, None);
        Ok(LocalResponse::ApprovalRequested {
            request_id: created.request_id,
            verification_words: protocol::pairing::verification_words(&Sha256::digest(&transcript)),
            expires_at: created.expires_at,
        })
    }

    /// Answer the control's callback for the open request (unauthenticated:
    /// it reveals only the secret's hash, which the control already holds).
    pub fn pairing_proof(&self, request_id: &str) -> Option<PairingProof> {
        if !is_request_id(request_id) {
            return None;
        }
        self.with_pending(|pending| {
            match &pending.request_id {
                Some(bound) if bound != request_id => return None,
                Some(_) => {}
                None => pending.request_id = Some(request_id.to_string()),
            }
            Some(PairingProof {
                request_id: request_id.to_string(),
                request_secret_sha256: pending.secret_sha256.clone(),
                control_machine_id: pending.control_machine_id.clone(),
            })
        })
        .ok()
        .flatten()
        .flatten()
    }

    /// Ask once whether the owner approved; save the link when they did.
    pub(super) async fn claim_approval(&self) -> Result<LocalResponse, LinkFailure> {
        let _operation = self.inner.operation.lock().await;
        let open = self.with_pending(|pending| {
            (
                pending.control_url.clone(),
                pending.request_id.clone(),
                pending.secret.clone(),
                pending.control_credential.clone(),
                pending.control_machine_id.clone(),
            )
        })?;
        let Some((control_url, Some(request_id), secret, control_credential, control_id)) = open
        else {
            return Err(LinkFailure::new(
                LocalErrorCode::NotFound,
                "no approval request is open",
            ));
        };
        let claim = PairingClaim {
            request_secret: Secret::new(secret),
            control_credential: Secret::new(control_credential.clone()),
        };
        let path = format!("/api/v1/pairing/requests/{request_id}/claim");
        let (status, body) = self
            .inner
            .client
            .send_raw(reqwest::Method::POST, &control_url, &path, &claim)
            .await
            .map_err(|error| {
                LinkFailure::new(LocalErrorCode::ControlUnreachable, error.to_string())
            })?;
        let result: PairingClaimResult = match serde_json::from_slice(&body) {
            Ok(result) => result,
            Err(_) => {
                let message = format!("the control refused the claim (HTTP {status})");
                return Err(self.abandon(LocalErrorCode::CodeRejected, &message));
            }
        };
        let link = match (result.state, result.link) {
            (PairingRequestState::Claimed, Some(link)) => link,
            (PairingRequestState::Pending | PairingRequestState::Approved, _) => {
                return Ok(LocalResponse::ApprovalState {
                    state: PairingRequestState::Pending,
                })
            }
            (state, _) => {
                self.set_pending(None)?;
                self.audit_link(
                    (self.inner.clock)(),
                    "APPROVAL_ENDED",
                    None,
                    Some(&format!("{state:?}")),
                );
                return Ok(LocalResponse::ApprovalState { state });
            }
        };
        if link.control_machine_id != control_id {
            return Err(self.abandon(
                LocalErrorCode::ControlUnreachable,
                "the claim came from a different control",
            ));
        }
        let now = (self.inner.clock)();
        let file = LinkFile::active(
            control_url,
            link.control_machine_id,
            link.link_id,
            link.worker_credential,
            &control_credential,
            link.credential_expires_at,
            link.display_name,
            now,
            link.heartbeat_interval_ms,
        );
        self.write_link(&file)?;
        self.set_pending(None)?;
        self.inner.failures.store(0, Ordering::SeqCst);
        self.audit_link(now, "CONNECTED", file.link_id.as_deref(), Some("APPROVAL"));
        drop(_operation);
        self.spawn_heartbeats();
        let machine_id = self.machine_id().map_err(identity_failure)?;
        self.report(Some(&file), &machine_id)
            .map(|connection| LocalResponse::Connection { connection })
    }

    /// Withdraw the open request (best effort on the control).
    pub(super) async fn cancel_approval(&self) -> Result<LocalResponse, LinkFailure> {
        let open = self.with_pending(|pending| {
            (
                pending.control_url.clone(),
                pending.request_id.clone(),
                pending.secret.clone(),
            )
        })?;
        self.set_pending(None)?;
        if let Some((control_url, Some(request_id), secret)) = open {
            let cancel = PairingCancel {
                request_secret: Secret::new(secret),
            };
            let path = format!("/api/v1/pairing/requests/{request_id}");
            let _ = self
                .inner
                .client
                .send_raw(reqwest::Method::DELETE, &control_url, &path, &cancel)
                .await;
        }
        Ok(LocalResponse::ApprovalState {
            state: PairingRequestState::Cancelled,
        })
    }

    fn set_pending(&self, value: Option<PendingApproval>) -> Result<(), LinkFailure> {
        *self
            .inner
            .approval
            .lock()
            .map_err(|_| internal("approval lock poisoned"))? = value;
        Ok(())
    }

    fn with_pending<T>(
        &self,
        f: impl FnOnce(&mut PendingApproval) -> T,
    ) -> Result<Option<T>, LinkFailure> {
        let mut guard = self
            .inner
            .approval
            .lock()
            .map_err(|_| internal("approval lock poisoned"))?;
        Ok(guard.as_mut().map(f))
    }

    fn abandon(&self, code: LocalErrorCode, message: &str) -> LinkFailure {
        let _ = self.set_pending(None);
        self.audit_link(
            (self.inner.clock)(),
            "APPROVAL_FAILED",
            None,
            Some(&format!("{code:?}")),
        );
        LinkFailure::new(code, message)
    }
}
