//! Click-to-approve pairing on the control (4.E §6). The claimed Machine ID
//! and name are never trusted: a request becomes approvable only after the
//! control calls the claimed Tailscale endpoint back over WireGuard (the
//! worker proves it holds the request) and `tailscale whois` names the node.
//! Rate limits are control-wide; `Tailscale-User-Login` is spoofable (F10).

use super::{
    approval_verify::{verification_words, PairingVerifier},
    ControlFailure, ControlService,
};
use crate::{link_audit::LinkAuditRecord, link_crypto};
use protocol::{
    link::{LinkFailureCode, RegistryStatus},
    pairing::{
        is_request_id, ExistingMachine, PairingApproval, PairingRequestCreate,
        PairingRequestCreated, PairingRequestList, PairingRequestState, PairingRequestView,
        TailscaleIdentity, PAIRING_POLL_MS, PAIRING_REQUEST_LIFETIME_MS,
    },
    MachineId, Timestamp,
};
use store::{pairing_requests::PairingRequestRow, registry::RegistryState};

/// PENDING requests at most.
const MAX_PENDING: u32 = 5;
/// New requests per window.
const MAX_REQUESTS_PER_WINDOW: u32 = 20;
/// Wrong-secret claims per window before every open request is cancelled.
pub(super) const MAX_CLAIM_FAILURES: u32 = 20;
/// Rate-limit window.
pub(super) const WINDOW_MS: i64 = 10 * 60 * 1000;
/// How far back the owner's list reaches.
const LIST_WINDOW_MS: i64 = 60 * 60 * 1000;

pub(super) fn failure(status: u16, code: LinkFailureCode, message: &str) -> ControlFailure {
    ControlFailure::new(status, code, message)
}

impl ControlService {
    /// Replace the callback/whois verifier (tests).
    pub fn with_pairing_verifier(mut self, verifier: std::sync::Arc<dyn PairingVerifier>) -> Self {
        self.verifier = verifier;
        self
    }

    /// Record a request, then verify it; an unverifiable request is cancelled.
    pub async fn create_pairing_request(
        &self,
        request: PairingRequestCreate,
    ) -> Result<PairingRequestCreated, ControlFailure> {
        let now = (self.clock)();
        let invalid = |message: &str| failure(400, LinkFailureCode::InvalidMachine, message);
        request
            .validate()
            .map_err(|error| invalid(&error.to_string()))?;
        if request.machine.machine_id == self.settings.machine_id {
            return Err(invalid("a worker cannot use the control's Machine ID"));
        }
        self.validate_endpoint(&request.endpoint).map_err(invalid)?;
        let since = Timestamp(now.0 - WINDOW_MS);
        let store = |error| ControlFailure::internal(error);
        if self
            .store
            .count_claim_failures_since(since)
            .map_err(store)?
            >= MAX_CLAIM_FAILURES
        {
            return Err(failure(
                429,
                LinkFailureCode::PairingLocked,
                "pairing is locked for a few minutes after too many wrong claims",
            ));
        }
        if self
            .store
            .count_pending_pairing_requests(now)
            .map_err(store)?
            >= MAX_PENDING
            || self
                .store
                .count_pairing_requests_since(since)
                .map_err(store)?
                >= MAX_REQUESTS_PER_WINDOW
        {
            return Err(failure(
                429,
                LinkFailureCode::PairingBusy,
                "too many pairing requests are waiting; try again later",
            ));
        }
        let machine_id = &request.machine.machine_id;
        if self
            .store
            .active_link_for(machine_id)
            .map_err(store)?
            .is_some()
        {
            self.audit.append(&LinkAuditRecord {
                machine_id: Some(&machine_id.0),
                ..LinkAuditRecord::new(now, "ALREADY_LINKED")
            });
            return Err(failure(
                409,
                LinkFailureCode::MachineAlreadyLinked,
                "this Machine is already connected; disconnect it first",
            ));
        }

        let request_id = link_crypto::new_handle("pr").map_err(ControlFailure::internal)?;
        let control_nonce =
            link_crypto::new_credential().map_err(ControlFailure::internal)?[..32].to_string();
        let expires_at = Timestamp(now.0 + PAIRING_REQUEST_LIFETIME_MS as i64);
        self.store
            .insert_pairing_request(&PairingRequestRow {
                request_id: request_id.clone(),
                created_at: now,
                expires_at,
                state: PairingRequestState::Pending,
                secret_sha256: request.request_secret_sha256.clone(),
                worker_nonce: request.worker_nonce.clone(),
                control_nonce: control_nonce.clone(),
                claimed: request.machine.clone(),
                endpoint: request.endpoint.clone(),
                tailscale: None,
                proof_ok: false,
                approved_name: None,
                decided_at: None,
                claimed_at: None,
            })
            .map_err(store)?;
        self.audit_request(now, "REQUESTED", &request_id, machine_id, None);

        match self.verify(&request_id, &request).await {
            Ok(identity) => {
                self.store
                    .verify_pairing_request(&request_id, &identity, (self.clock)())
                    .map_err(store)?;
                self.audit_request(
                    now,
                    "PROOF_OK",
                    &request_id,
                    machine_id,
                    Some(&identity.node),
                );
            }
            Err(reason) => {
                let _ = self.store.cancel_pairing_request(
                    &request_id,
                    &request.request_secret_sha256,
                    now,
                );
                self.audit_request(now, "PROOF_FAILED", &request_id, machine_id, Some(reason));
                return Err(failure(
                    403,
                    LinkFailureCode::InvalidMachine,
                    "the control could not verify this Machine over Tailscale; check that sentinald is running and reachable on its Tailscale address",
                ));
            }
        }
        Ok(PairingRequestCreated {
            request_id,
            control_machine_id: self.settings.machine_id.clone(),
            control_nonce,
            expires_at,
            poll_after_ms: PAIRING_POLL_MS,
        })
    }

    /// The callback proof, then `whois` on the same address.
    async fn verify(
        &self,
        request_id: &str,
        request: &PairingRequestCreate,
    ) -> Result<TailscaleIdentity, &'static str> {
        let proof = self
            .verifier
            .proof(&request.endpoint, request_id)
            .await
            .map_err(|_| "CALLBACK_FAILED")?;
        if proof.request_id != request_id
            || proof.request_secret_sha256 != request.request_secret_sha256
            || proof.control_machine_id != self.settings.machine_id
        {
            return Err("PROOF_MISMATCH");
        }
        let identity = self
            .verifier
            .whois(&request.endpoint.address)
            .await
            .map_err(|_| "WHOIS_FAILED")?;
        if identity.address != request.endpoint.address {
            return Err("WHOIS_MISMATCH");
        }
        Ok(identity)
    }

    /// Recent requests for the owner, newest first.
    pub fn pairing_requests(&self) -> Result<PairingRequestList, ControlFailure> {
        let now = (self.clock)();
        let rows = self
            .store
            .recent_pairing_requests(Timestamp(now.0 - LIST_WINDOW_MS), 20)
            .map_err(ControlFailure::internal)?;
        let requests = rows
            .into_iter()
            .map(|row| self.view(row, now))
            .collect::<Result<_, _>>()?;
        Ok(PairingRequestList { requests })
    }

    fn view(
        &self,
        row: PairingRequestRow,
        now: Timestamp,
    ) -> Result<PairingRequestView, ControlFailure> {
        let existing_machine = self
            .store
            .registry_machine(&row.claimed.machine_id)
            .map_err(ControlFailure::internal)?
            .filter(|machine| machine.state != RegistryState::Active)
            .map(|machine| ExistingMachine {
                display_name: machine.display_name,
                status: match machine.state {
                    RegistryState::Revoked => RegistryStatus::Revoked,
                    _ => RegistryStatus::Disconnected,
                },
                last_seen: machine.last_seen,
            });
        Ok(PairingRequestView {
            verification_words: verification_words(
                &self.settings.machine_id,
                &row.request_id,
                &row.control_nonce,
                &row.secret_sha256,
                &row.worker_nonce,
            ),
            state: row.state_at(now),
            verified: row.proof_ok,
            request_id: row.request_id,
            expires_at: row.expires_at,
            claimed: row.claimed,
            tailscale: row.tailscale,
            existing_machine,
        })
    }

    /// Owner approval of one verified request.
    pub fn approve_pairing_request(
        &self,
        request_id: &str,
        approval: &PairingApproval,
    ) -> Result<PairingRequestState, ControlFailure> {
        let now = (self.clock)();
        let row = self.request_row(request_id)?;
        let name = match &approval.display_name {
            Some(name) => protocol::link::validate_display_name(name).map_err(|error| {
                failure(400, LinkFailureCode::InvalidRequest, &error.to_string())
            })?,
            None => row.claimed.display_name.clone(),
        };
        if !self
            .store
            .decide_pairing_request(request_id, Some(&name), now)
            .map_err(ControlFailure::internal)?
        {
            return Err(failure(
                409,
                LinkFailureCode::InvalidRequest,
                "only a verified request that is still waiting can be approved",
            ));
        }
        self.audit_request(now, "APPROVED", request_id, &row.claimed.machine_id, None);
        Ok(PairingRequestState::Approved)
    }

    /// Owner rejection.
    pub fn reject_pairing_request(
        &self,
        request_id: &str,
    ) -> Result<PairingRequestState, ControlFailure> {
        let now = (self.clock)();
        let row = self.request_row(request_id)?;
        if !self
            .store
            .decide_pairing_request(request_id, None, now)
            .map_err(ControlFailure::internal)?
        {
            return Err(failure(
                409,
                LinkFailureCode::InvalidRequest,
                "this request is no longer open",
            ));
        }
        self.audit_request(now, "REJECTED", request_id, &row.claimed.machine_id, None);
        Ok(PairingRequestState::Rejected)
    }

    pub(super) fn request_row(
        &self,
        request_id: &str,
    ) -> Result<PairingRequestRow, ControlFailure> {
        let unknown = || failure(404, LinkFailureCode::NotFound, "unknown pairing request");
        if !is_request_id(request_id) {
            return Err(unknown());
        }
        self.store
            .pairing_request(request_id)
            .map_err(ControlFailure::internal)?
            .ok_or_else(unknown)
    }

    pub(super) fn audit_request(
        &self,
        now: Timestamp,
        kind: &str,
        request_id: &str,
        machine_id: &MachineId,
        reason: Option<&str>,
    ) {
        self.audit.append(&LinkAuditRecord {
            request_id: Some(request_id),
            machine_id: Some(&machine_id.0),
            reason,
            ..LinkAuditRecord::new(now, kind)
        });
    }
}
