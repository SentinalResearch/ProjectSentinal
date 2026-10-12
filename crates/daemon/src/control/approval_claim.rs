//! The worker's side of approval pairing on the control: claim the approved
//! credential exactly once, or withdraw the request, with the request secret.

use super::{
    approval::{failure, MAX_CLAIM_FAILURES, WINDOW_MS},
    secrets::PresentedCredential,
    ControlFailure, ControlService, CREDENTIAL_LIFETIME_MS,
};
use crate::{
    link_audit::LinkAuditRecord,
    link_crypto::{self, sha256_hex},
};
use protocol::{
    link::{LinkExchangeResponse, LinkFailureCode, MachineRole, Secret},
    pairing::{
        PairingCancel, PairingClaim, PairingClaimResult, PairingRequestState, PAIRING_POLL_MS,
    },
    MachineId, Timestamp,
};
use store::registry::{LinkRow, RegistryRow, RegistryState};

impl ControlService {
    /// Worker withdrawal, with the request secret.
    pub fn cancel_pairing_request(
        &self,
        request_id: &str,
        cancel: &PairingCancel,
    ) -> Result<(), ControlFailure> {
        let now = (self.clock)();
        let row = self.request_row(request_id)?;
        let hash = sha256_hex(cancel.request_secret.expose());
        if hash != row.secret_sha256 {
            return Err(self.wrong_claim(now, request_id, &row.claimed.machine_id)?);
        }
        self.store
            .cancel_pairing_request(request_id, &hash, now)
            .map_err(ControlFailure::internal)?;
        self.audit_request(now, "CANCELLED", request_id, &row.claimed.machine_id, None);
        Ok(())
    }

    /// Worker claim: the approved credential, exactly once.
    pub fn claim_pairing_request(
        &self,
        request_id: &str,
        claim: &PairingClaim,
    ) -> Result<PairingClaimResult, ControlFailure> {
        let now = (self.clock)();
        let row = self.request_row(request_id)?;
        let hash = sha256_hex(claim.request_secret.expose());
        if hash != row.secret_sha256 {
            return Err(self.wrong_claim(now, request_id, &row.claimed.machine_id)?);
        }
        let ended = |state| {
            Ok(PairingClaimResult {
                state,
                poll_after_ms: None,
                link: None,
            })
        };
        match row.state_at(now) {
            PairingRequestState::Pending => {
                return Ok(PairingClaimResult {
                    state: PairingRequestState::Pending,
                    poll_after_ms: Some(PAIRING_POLL_MS),
                    link: None,
                })
            }
            PairingRequestState::Approved => {}
            state => return ended(state),
        }
        if !link_crypto::is_credential(claim.control_credential.expose()) {
            return Err(failure(
                400,
                LinkFailureCode::InvalidRequest,
                "control_credential must be 64 hex characters",
            ));
        }
        let machine_id = row.claimed.machine_id.clone();
        let worker_credential = link_crypto::new_credential().map_err(ControlFailure::internal)?;
        let link_id = link_crypto::new_handle("lk").map_err(ControlFailure::internal)?;
        let expires_at = Timestamp(now.0 + CREDENTIAL_LIFETIME_MS);
        // Secret first: a committed link must always have its credential.
        self.with_secrets(|secrets| {
            secrets.insert(
                &link_id,
                PresentedCredential {
                    machine_id: machine_id.clone(),
                    control_credential: claim.control_credential.clone(),
                },
            )
        })?;
        let machine = RegistryRow {
            machine_id: machine_id.clone(),
            display_name: row
                .approved_name
                .clone()
                .unwrap_or_else(|| row.claimed.display_name.clone()),
            role: MachineRole::Worker,
            os: Some(row.claimed.os),
            daemon_version: Some(row.claimed.daemon_version.clone()),
            capabilities: row.claimed.capabilities.clone(),
            endpoint: Some(row.endpoint.clone()),
            state: RegistryState::Active,
            paired_at: now,
            last_seen: None,
            last_sequence: 0,
        };
        let link = LinkRow {
            link_id: link_id.clone(),
            machine_id: machine_id.clone(),
            worker_credential_sha256: sha256_hex(&worker_credential),
            created_at: now,
            expires_at,
            previous_sha256: None,
            previous_valid_until: None,
            revoked_at: None,
            last_used_at: None,
        };
        let outcome = match self
            .store
            .claim_pairing_request(request_id, &hash, &machine, &link, now)
        {
            Ok(Some(outcome)) => outcome,
            Ok(None) => {
                self.with_secrets(|secrets| secrets.remove(std::slice::from_ref(&link_id)))?;
                return ended(PairingRequestState::Expired);
            }
            Err(error) => {
                let _ = self.with_secrets(|secrets| secrets.remove(std::slice::from_ref(&link_id)));
                return Err(ControlFailure::internal(error));
            }
        };
        if !outcome.replaced_links.is_empty() {
            self.with_secrets(|secrets| secrets.remove(&outcome.replaced_links))?;
        }
        self.audit.append(&LinkAuditRecord {
            request_id: Some(request_id),
            machine_id: Some(&machine_id.0),
            link_id: Some(&link_id),
            ..LinkAuditRecord::new(now, "CLAIMED")
        });
        Ok(PairingClaimResult {
            state: PairingRequestState::Claimed,
            poll_after_ms: None,
            link: Some(LinkExchangeResponse {
                link_id,
                control_machine_id: self.settings.machine_id.clone(),
                worker_credential: Secret::new(worker_credential),
                credential_expires_at: expires_at,
                display_name: outcome.display_name,
                heartbeat_interval_ms: self.settings.heartbeat_interval_ms,
            }),
        })
    }

    /// Count a wrong secret; the failure that reaches the limit cancels every
    /// open request.
    fn wrong_claim(
        &self,
        now: Timestamp,
        request_id: &str,
        machine_id: &MachineId,
    ) -> Result<ControlFailure, ControlFailure> {
        self.store
            .record_claim_failure(now)
            .map_err(ControlFailure::internal)?;
        self.audit_request(
            now,
            "CLAIM_REFUSED",
            request_id,
            machine_id,
            Some("WRONG_SECRET"),
        );
        let since = Timestamp(now.0 - WINDOW_MS);
        if self
            .store
            .count_claim_failures_since(since)
            .map_err(ControlFailure::internal)?
            >= MAX_CLAIM_FAILURES
        {
            self.store
                .cancel_open_pairing_requests(now)
                .map_err(ControlFailure::internal)?;
            self.audit.append(&LinkAuditRecord {
                reason: Some("TOO_MANY_WRONG_CLAIMS"),
                ..LinkAuditRecord::new(now, "PAIRING_LOCKED")
            });
            return Ok(failure(
                429,
                LinkFailureCode::PairingLocked,
                "pairing is locked for a few minutes after too many wrong claims",
            ));
        }
        Ok(failure(
            401,
            LinkFailureCode::InvalidClaim,
            "the request secret does not match",
        ))
    }
}
