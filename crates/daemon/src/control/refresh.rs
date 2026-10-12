//! Worker credential refresh (F5). Two stores change: the owner-only secrets
//! file (the control credential presented to the worker) and SQLite (the hash
//! of the worker's credential). The secrets file first records how to undo
//! the refresh, atomically with the new credential; any later failure, or a
//! crash before the refresh is committed, restores both sides, because the
//! worker never received its new credential.

use super::{
    secrets::{PendingRefresh, PresentedCredential},
    ControlFailure, ControlService, CREDENTIAL_LIFETIME_MS, REFRESH_GRACE_MS,
};
use crate::{link_audit::LinkAuditRecord, link_crypto};
use protocol::{
    link::{LinkFailureCode, LinkRefreshRequest, LinkRefreshResponse},
    Secret, Timestamp,
};

/// Failures a test can inject at each refresh boundary.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshFault {
    /// The SQLite rotation fails after the secrets file changed.
    RotationFails,
    /// The daemon stops after both writes but before committing.
    CrashBeforeCommit,
}

impl ControlService {
    /// Rotate both directional credentials; the worker keeps the old one for
    /// a short grace period.
    pub fn refresh(
        &self,
        bearer: &str,
        request: LinkRefreshRequest,
    ) -> Result<LinkRefreshResponse, ControlFailure> {
        let now = (self.clock)();
        let link = self.authenticate(bearer, &request.machine_id, now)?;
        if !link_crypto::is_credential(request.control_credential.expose()) {
            return Err(ControlFailure::new(
                400,
                LinkFailureCode::InvalidRequest,
                "control_credential must be 64 hex characters",
            ));
        }
        let worker_credential = link_crypto::new_credential().map_err(ControlFailure::internal)?;
        let expires_at = Timestamp(now.0 + CREDENTIAL_LIFETIME_MS);
        let fault = self.refresh_fault.lock().ok().and_then(|fault| *fault);

        self.with_secrets(|secrets| {
            let previous = secrets.credential(&link.link_id).cloned();
            secrets.begin_refresh(
                PresentedCredential {
                    machine_id: request.machine_id.clone(),
                    control_credential: request.control_credential.clone(),
                },
                PendingRefresh {
                    link_id: link.link_id.clone(),
                    previous,
                    previous_sha256: link.worker_credential_sha256.clone(),
                    previous_expires_at: link.expires_at,
                },
            )
        })?;

        let rotated = if fault == Some(RefreshFault::RotationFails) {
            Err("injected rotation failure".to_string())
        } else {
            self.store
                .rotate_link(
                    &link.link_id,
                    &link_crypto::sha256_hex(&worker_credential),
                    expires_at,
                    Timestamp(now.0 + REFRESH_GRACE_MS),
                    now,
                )
                .map_err(|error| error.to_string())
        };
        match rotated {
            Ok(true) => {}
            Ok(false) => {
                self.reconcile_refresh()?;
                return Err(ControlFailure::internal(
                    "the link changed during refresh; nothing was rotated",
                ));
            }
            Err(error) => {
                self.reconcile_refresh()?;
                return Err(ControlFailure::internal(error));
            }
        }
        if fault == Some(RefreshFault::CrashBeforeCommit) {
            return Err(ControlFailure::internal("injected crash before commit"));
        }
        if let Err(failure) = self.with_secrets(|secrets| secrets.commit_refresh()) {
            // Not committed means not delivered: undo rather than strand the worker.
            self.reconcile_refresh()?;
            return Err(failure);
        }
        self.audit.append(&LinkAuditRecord {
            machine_id: Some(&request.machine_id.0),
            link_id: Some(&link.link_id),
            ..LinkAuditRecord::new(now, "CREDENTIAL_REFRESHED")
        });
        Ok(LinkRefreshResponse {
            worker_credential: Secret::new(worker_credential),
            credential_expires_at: expires_at,
        })
    }

    /// Undo a refresh that never committed: restore the worker's previous
    /// credential hash (the new one is invalid immediately) and the control
    /// credential presented before it. Idempotent; runs at startup.
    pub(super) fn reconcile_refresh(&self) -> Result<(), ControlFailure> {
        let pending = self
            .secrets
            .lock()
            .map_err(|_| ControlFailure::internal("control secrets lock poisoned"))?
            .pending_refresh();
        let Some(pending) = pending else {
            return Ok(());
        };
        let now = (self.clock)();
        self.store
            .rotate_link(
                &pending.link_id,
                &pending.previous_sha256,
                pending.previous_expires_at,
                now,
                now,
            )
            .map_err(ControlFailure::internal)?;
        self.with_secrets(|secrets| secrets.abort_refresh())?;
        self.audit.append(&LinkAuditRecord {
            machine_id: pending
                .previous
                .as_ref()
                .map(|credential| credential.machine_id.0.as_str()),
            link_id: Some(&pending.link_id),
            ..LinkAuditRecord::new(now, "CREDENTIAL_REFRESH_ROLLED_BACK")
        });
        Ok(())
    }

    /// Make the next refreshes fail at one boundary (fault-injection tests only).
    #[doc(hidden)]
    pub fn inject_refresh_fault(&self, fault: Option<RefreshFault>) {
        if let Ok(mut slot) = self.refresh_fault.lock() {
            *slot = fault;
        }
    }
}
