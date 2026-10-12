//! Click-to-approve pairing requests (4.E §6). A request is approvable only
//! after its callback proof and `tailscale whois` succeed, and claimable once,
//! with the secret whose hash it was created with, in the same transaction as
//! the registry and link writes.

use crate::{
    registry::{write_link, LinkRow, RegistryRow},
    Store, StoreError,
};
use protocol::{
    link::{LinkEndpoint, LinkMachineInfo},
    pairing::{PairingRequestState, TailscaleIdentity},
    Timestamp,
};
use rusqlite::{params, OptionalExtension, Row};

/// One stored request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingRequestRow {
    /// `pr_` handle.
    pub request_id: String,
    /// Creation time.
    pub created_at: Timestamp,
    /// Expiry.
    pub expires_at: Timestamp,
    /// Stored state (`PENDING` past expiry reads as expired; see [`Self::state_at`]).
    pub state: PairingRequestState,
    /// SHA-256 of the worker's request secret.
    pub secret_sha256: String,
    /// Worker nonce.
    pub worker_nonce: String,
    /// Control nonce.
    pub control_nonce: String,
    /// Claimed identity (untrusted).
    pub claimed: LinkMachineInfo,
    /// Callback endpoint.
    pub endpoint: LinkEndpoint,
    /// Tailscale's identity, once verified.
    pub tailscale: Option<TailscaleIdentity>,
    /// Whether the callback proof and `whois` both succeeded.
    pub proof_ok: bool,
    /// Owner-chosen name on approval.
    pub approved_name: Option<String>,
    /// When the owner decided.
    pub decided_at: Option<Timestamp>,
    /// When the worker claimed.
    pub claimed_at: Option<Timestamp>,
}

impl PairingRequestRow {
    /// The state as of `now`: an unclaimed PENDING or APPROVED request past
    /// its expiry is EXPIRED.
    pub fn state_at(&self, now: Timestamp) -> PairingRequestState {
        match self.state {
            PairingRequestState::Pending | PairingRequestState::Approved
                if now.0 >= self.expires_at.0 =>
            {
                PairingRequestState::Expired
            }
            state => state,
        }
    }
}

/// What a successful claim wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimOutcome {
    /// Earlier links of the same Machine that were revoked.
    pub replaced_links: Vec<String>,
    /// Display name now recorded.
    pub display_name: String,
}

fn state_text(state: PairingRequestState) -> &'static str {
    match state {
        PairingRequestState::Pending => "PENDING",
        PairingRequestState::Approved => "APPROVED",
        PairingRequestState::Rejected => "REJECTED",
        PairingRequestState::Cancelled => "CANCELLED",
        PairingRequestState::Expired => "EXPIRED",
        PairingRequestState::Claimed => "CLAIMED",
    }
}

fn parse_state(text: &str) -> rusqlite::Result<PairingRequestState> {
    Ok(match text {
        "PENDING" => PairingRequestState::Pending,
        "APPROVED" => PairingRequestState::Approved,
        "REJECTED" => PairingRequestState::Rejected,
        "CANCELLED" => PairingRequestState::Cancelled,
        "EXPIRED" => PairingRequestState::Expired,
        "CLAIMED" => PairingRequestState::Claimed,
        other => {
            return Err(rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                format!("unknown pairing request state {other}").into(),
            ))
        }
    })
}

fn json<T: serde::de::DeserializeOwned>(text: &str) -> rusqlite::Result<T> {
    serde_json::from_str(text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
    })
}

fn request_row(row: &Row<'_>) -> rusqlite::Result<PairingRequestRow> {
    let tailscale: Option<String> = row.get("tailscale_json")?;
    Ok(PairingRequestRow {
        request_id: row.get("request_id")?,
        created_at: Timestamp(row.get("created_at")?),
        expires_at: Timestamp(row.get("expires_at")?),
        state: parse_state(&row.get::<_, String>("state")?)?,
        secret_sha256: row.get("secret_sha256")?,
        worker_nonce: row.get("worker_nonce")?,
        control_nonce: row.get("control_nonce")?,
        claimed: json(&row.get::<_, String>("claimed_machine_json")?)?,
        endpoint: json(&row.get::<_, String>("endpoint_json")?)?,
        tailscale: tailscale.as_deref().map(json).transpose()?,
        proof_ok: row.get("proof_ok")?,
        approved_name: row.get("approved_name")?,
        decided_at: row.get::<_, Option<i64>>("decided_at")?.map(Timestamp),
        claimed_at: row.get::<_, Option<i64>>("claimed_at")?.map(Timestamp),
    })
}

impl Store {
    /// Record a new PENDING request.
    pub fn insert_pairing_request(&self, request: &PairingRequestRow) -> Result<(), StoreError> {
        self.lock()?.execute(
            "INSERT INTO pairing_requests(request_id, created_at, expires_at, state, secret_sha256,
                 worker_nonce, control_nonce, claimed_machine_json, endpoint_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                request.request_id,
                request.created_at.0,
                request.expires_at.0,
                state_text(request.state),
                request.secret_sha256,
                request.worker_nonce,
                request.control_nonce,
                serde_json::to_string(&request.claimed)?,
                serde_json::to_string(&request.endpoint)?,
            ],
        )?;
        Ok(())
    }

    /// One request.
    pub fn pairing_request(
        &self,
        request_id: &str,
    ) -> Result<Option<PairingRequestRow>, StoreError> {
        Ok(self
            .lock()?
            .query_row(
                "SELECT * FROM pairing_requests WHERE request_id = ?1",
                [request_id],
                request_row,
            )
            .optional()?)
    }

    /// Requests created since `since`, newest first, at most `limit`.
    pub fn recent_pairing_requests(
        &self,
        since: Timestamp,
        limit: u32,
    ) -> Result<Vec<PairingRequestRow>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT * FROM pairing_requests WHERE created_at >= ?1
             ORDER BY created_at DESC, request_id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![since.0, limit], request_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// PENDING requests that have not expired.
    pub fn count_pending_pairing_requests(&self, now: Timestamp) -> Result<u32, StoreError> {
        Ok(self.lock()?.query_row(
            "SELECT COUNT(*) FROM pairing_requests WHERE state = 'PENDING' AND expires_at > ?1",
            [now.0],
            |row| row.get(0),
        )?)
    }

    /// Requests created since `since`.
    pub fn count_pairing_requests_since(&self, since: Timestamp) -> Result<u32, StoreError> {
        Ok(self.lock()?.query_row(
            "SELECT COUNT(*) FROM pairing_requests WHERE created_at >= ?1",
            [since.0],
            |row| row.get(0),
        )?)
    }

    /// Mark a PENDING request verified with Tailscale's identity.
    pub fn verify_pairing_request(
        &self,
        request_id: &str,
        tailscale: &TailscaleIdentity,
        now: Timestamp,
    ) -> Result<bool, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE pairing_requests SET proof_ok = 1, tailscale_json = ?2
             WHERE request_id = ?1 AND state = 'PENDING' AND expires_at > ?3",
            params![request_id, serde_json::to_string(tailscale)?, now.0],
        )? == 1)
    }

    /// Approve (only a verified, unexpired PENDING request) or reject it.
    pub fn decide_pairing_request(
        &self,
        request_id: &str,
        approve: Option<&str>,
        now: Timestamp,
    ) -> Result<bool, StoreError> {
        let changed = match approve {
            Some(name) => self.lock()?.execute(
                "UPDATE pairing_requests SET state = 'APPROVED', approved_name = ?2, decided_at = ?3
                 WHERE request_id = ?1 AND state = 'PENDING' AND proof_ok = 1 AND expires_at > ?3",
                params![request_id, name, now.0],
            )?,
            None => self.lock()?.execute(
                "UPDATE pairing_requests SET state = 'REJECTED', decided_at = ?2
                 WHERE request_id = ?1 AND state IN ('PENDING', 'APPROVED') AND expires_at > ?2",
                params![request_id, now.0],
            )?,
        };
        Ok(changed == 1)
    }

    /// Cancel an open request whose secret hash matches.
    pub fn cancel_pairing_request(
        &self,
        request_id: &str,
        secret_sha256: &str,
        now: Timestamp,
    ) -> Result<bool, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE pairing_requests SET state = 'CANCELLED', decided_at = ?3
             WHERE request_id = ?1 AND secret_sha256 = ?2 AND state IN ('PENDING', 'APPROVED')",
            params![request_id, secret_sha256, now.0],
        )? == 1)
    }

    /// Cancel every open request (claim lockout); returns how many.
    pub fn cancel_open_pairing_requests(&self, now: Timestamp) -> Result<usize, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE pairing_requests SET state = 'CANCELLED', decided_at = ?1
             WHERE state IN ('PENDING', 'APPROVED')",
            [now.0],
        )?)
    }

    /// Claim an approved request exactly once and write the Machine and its
    /// link in the same transaction. `None` when nothing matched.
    pub fn claim_pairing_request(
        &self,
        request_id: &str,
        secret_sha256: &str,
        machine: &RegistryRow,
        link: &LinkRow,
        now: Timestamp,
    ) -> Result<Option<ClaimOutcome>, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let claimed = transaction.execute(
            "UPDATE pairing_requests SET state = 'CLAIMED', claimed_at = ?3
             WHERE request_id = ?1 AND secret_sha256 = ?2 AND state = 'APPROVED'
               AND claimed_at IS NULL AND expires_at > ?3",
            params![request_id, secret_sha256, now.0],
        )?;
        if claimed != 1 {
            return Ok(None);
        }
        let (replaced_links, display_name) = write_link(&transaction, machine, link, now, true)?;
        transaction.commit()?;
        Ok(Some(ClaimOutcome {
            replaced_links,
            display_name,
        }))
    }

    /// Record a wrong-secret claim or cancel.
    pub fn record_claim_failure(&self, at: Timestamp) -> Result<(), StoreError> {
        self.lock()?
            .execute("INSERT INTO pairing_claim_failures(at) VALUES (?1)", [at.0])?;
        Ok(())
    }

    /// Wrong-secret claims since `since`.
    pub fn count_claim_failures_since(&self, since: Timestamp) -> Result<u32, StoreError> {
        Ok(self.lock()?.query_row(
            "SELECT COUNT(*) FROM pairing_claim_failures WHERE at >= ?1",
            [since.0],
            |row| row.get(0),
        )?)
    }
}
