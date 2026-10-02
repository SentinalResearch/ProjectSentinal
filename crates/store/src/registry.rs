//! Control-Machine registry, pairing codes, worker links, and Machine ID rewrite.
//!
//! The store never sees a secret: codes are kept as keyed HMACs and worker
//! credentials as SHA-256 hashes computed by the daemon.

use crate::{to_sql_id, Store, StoreError};
use protocol::{
    link::{LinkEndpoint, MachineCapability, MachineRole},
    MachineId, Os, Timestamp,
};
use rusqlite::{params, OptionalExtension, Row};

/// Pairing failures older than this are pruned when a new one is recorded.
const FAILURE_RETENTION_MS: i64 = 24 * 60 * 60 * 1_000;

/// One stored pairing code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingCodeRow {
    /// Non-secret handle.
    pub code_id: String,
    /// Keyed HMAC of the normalized code.
    pub code_hmac: String,
    /// Creation time.
    pub created_at: Timestamp,
    /// Expiry (exclusive).
    pub expires_at: Timestamp,
    /// When a worker exchanged it.
    pub consumed_at: Option<Timestamp>,
    /// When it was cancelled.
    pub cancelled_at: Option<Timestamp>,
    /// Machine that exchanged it.
    pub machine_id: Option<MachineId>,
}

/// Persisted registry lifecycle (online/offline is derived from `last_seen`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryState {
    /// Linked.
    Active,
    /// The worker disconnected itself.
    Disconnected,
    /// The owner revoked it.
    Revoked,
}

impl RegistryState {
    fn as_str(self) -> &'static str {
        match self {
            RegistryState::Active => "ACTIVE",
            RegistryState::Disconnected => "DISCONNECTED",
            RegistryState::Revoked => "REVOKED",
        }
    }

    fn parse(value: &str) -> Result<Self, StoreError> {
        match value {
            "ACTIVE" => Ok(RegistryState::Active),
            "DISCONNECTED" => Ok(RegistryState::Disconnected),
            "REVOKED" => Ok(RegistryState::Revoked),
            other => Err(StoreError::InvalidRegistryValue {
                field: "state",
                value: other.to_string(),
            }),
        }
    }
}

/// One Machine in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryRow {
    /// Immutable Machine ID.
    pub machine_id: MachineId,
    /// Owner-editable display name.
    pub display_name: String,
    /// Control or worker.
    pub role: MachineRole,
    /// Reported OS.
    pub os: Option<Os>,
    /// Reported daemon version.
    pub daemon_version: Option<String>,
    /// Reported capabilities.
    pub capabilities: Vec<MachineCapability>,
    /// Latest reported endpoint.
    pub endpoint: Option<LinkEndpoint>,
    /// Lifecycle state.
    pub state: RegistryState,
    /// When it was first paired.
    pub paired_at: Timestamp,
    /// Last accepted heartbeat.
    pub last_seen: Option<Timestamp>,
    /// Last accepted heartbeat sequence.
    pub last_sequence: u64,
}

/// One worker link (credential hashes only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRow {
    /// Link handle.
    pub link_id: String,
    /// Linked Machine.
    pub machine_id: MachineId,
    /// SHA-256 of the current worker credential.
    pub worker_credential_sha256: String,
    /// When the current credential was issued.
    pub created_at: Timestamp,
    /// When the current credential expires.
    pub expires_at: Timestamp,
    /// SHA-256 of the credential replaced by the last refresh.
    pub previous_sha256: Option<String>,
    /// End of the previous credential's grace period.
    pub previous_valid_until: Option<Timestamp>,
    /// When the link was revoked.
    pub revoked_at: Option<Timestamp>,
    /// Last accepted heartbeat.
    pub last_used_at: Option<Timestamp>,
}

/// Everything a successful exchange writes, applied in one transaction.
#[derive(Debug, Clone)]
pub struct ExchangeRecord {
    /// HMAC of the presented code.
    pub code_hmac: String,
    /// Exchange time.
    pub now: Timestamp,
    /// Registry entry (an existing owner display name is kept).
    pub machine: RegistryRow,
    /// New link.
    pub link: LinkRow,
}

/// Result of a successful exchange.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeOutcome {
    /// Consumed code.
    pub code_id: String,
    /// Earlier links of the same Machine that were revoked.
    pub replaced_links: Vec<String>,
    /// Display name now recorded.
    pub display_name: String,
}

impl Store {
    /// Store a new pairing code.
    pub fn insert_pairing_code(&self, code: &PairingCodeRow) -> Result<(), StoreError> {
        self.lock()?.execute(
            "INSERT INTO pairing_codes(code_id, code_hmac, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![code.code_id, code.code_hmac, code.created_at.0, code.expires_at.0],
        )?;
        Ok(())
    }

    /// Return one pairing code by handle.
    pub fn pairing_code(&self, code_id: &str) -> Result<Option<PairingCodeRow>, StoreError> {
        Ok(self
            .lock()?
            .query_row(
                "SELECT code_id, code_hmac, created_at, expires_at, consumed_at, cancelled_at, machine_id
                 FROM pairing_codes WHERE code_id = ?1",
                [code_id],
                code_row,
            )
            .optional()?)
    }

    /// Codes created at or after `since`.
    pub fn count_codes_created_since(&self, since: Timestamp) -> Result<u32, StoreError> {
        count(
            self,
            "SELECT COUNT(*) FROM pairing_codes WHERE created_at >= ?1",
            since,
        )
    }

    /// Unused, uncancelled, unexpired codes at `now`.
    pub fn count_outstanding_codes(&self, now: Timestamp) -> Result<u32, StoreError> {
        count(
            self,
            "SELECT COUNT(*) FROM pairing_codes
             WHERE consumed_at IS NULL AND cancelled_at IS NULL AND expires_at > ?1",
            now,
        )
    }

    /// Cancel one outstanding code; `false` when it was not outstanding.
    pub fn cancel_pairing_code(&self, code_id: &str, now: Timestamp) -> Result<bool, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE pairing_codes SET cancelled_at = ?2
             WHERE code_id = ?1 AND consumed_at IS NULL AND cancelled_at IS NULL AND expires_at > ?2",
            params![code_id, now.0],
        )? == 1)
    }

    /// Cancel every outstanding code, returning how many were cancelled.
    pub fn cancel_outstanding_codes(&self, now: Timestamp) -> Result<usize, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE pairing_codes SET cancelled_at = ?1
             WHERE consumed_at IS NULL AND cancelled_at IS NULL AND expires_at > ?1",
            [now.0],
        )?)
    }

    /// Mark unused codes that expired at or before `now`, once each, returning their IDs.
    pub fn take_newly_expired_codes(&self, now: Timestamp) -> Result<Vec<String>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "UPDATE pairing_codes SET expiry_audited_at = ?1
             WHERE consumed_at IS NULL AND cancelled_at IS NULL
               AND expires_at <= ?1 AND expiry_audited_at IS NULL
             RETURNING code_id",
        )?;
        let rows = statement.query_map([now.0], |row| row.get(0))?;
        Ok(rows.collect::<Result<Vec<String>, _>>()?)
    }

    /// Record one failed exchange for the control-wide lockout window.
    pub fn record_pairing_failure(&self, at: Timestamp) -> Result<(), StoreError> {
        let connection = self.lock()?;
        connection.execute("INSERT INTO pairing_failures(at) VALUES (?1)", [at.0])?;
        connection.execute(
            "DELETE FROM pairing_failures WHERE at < ?1",
            [at.0.saturating_sub(FAILURE_RETENTION_MS)],
        )?;
        Ok(())
    }

    /// Failed exchanges at or after `since`.
    pub fn count_pairing_failures_since(&self, since: Timestamp) -> Result<u32, StoreError> {
        count(
            self,
            "SELECT COUNT(*) FROM pairing_failures WHERE at >= ?1",
            since,
        )
    }

    /// Consume a code and write the registry entry and link atomically.
    ///
    /// Returns `None` when the code is unknown, used, cancelled, or expired.
    pub fn complete_exchange(
        &self,
        record: &ExchangeRecord,
    ) -> Result<Option<ExchangeOutcome>, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let machine_id = &record.machine.machine_id.0;
        let consumed = transaction
            .query_row(
                "UPDATE pairing_codes SET consumed_at = ?2, machine_id = ?3
                 WHERE code_hmac = ?1 AND consumed_at IS NULL AND cancelled_at IS NULL AND expires_at > ?2
                 RETURNING code_id",
                params![record.code_hmac, record.now.0, machine_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(code_id) = consumed else {
            return Ok(None);
        };
        let replaced_links = {
            let mut statement = transaction.prepare(
                "UPDATE registry_links SET revoked_at = ?2
                 WHERE machine_id = ?1 AND revoked_at IS NULL RETURNING link_id",
            )?;
            let rows = statement.query_map(params![machine_id, record.now.0], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        let machine = &record.machine;
        transaction.execute(
            "INSERT INTO registry_machines(machine_id, display_name, role, os, daemon_version,
                 capabilities_json, endpoint_json, state, paired_at, last_seen, last_sequence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, 0)
             ON CONFLICT(machine_id) DO UPDATE SET
                 os = excluded.os, daemon_version = excluded.daemon_version,
                 capabilities_json = excluded.capabilities_json, endpoint_json = excluded.endpoint_json,
                 state = excluded.state, last_sequence = 0",
            params![
                machine_id,
                machine.display_name,
                role_text(machine.role),
                machine.os.map(|os| serde_json::to_string(&os)).transpose()?,
                machine.daemon_version,
                serde_json::to_string(&machine.capabilities)?,
                machine.endpoint.as_ref().map(serde_json::to_string).transpose()?,
                RegistryState::Active.as_str(),
                machine.paired_at.0,
            ],
        )?;
        let link = &record.link;
        transaction.execute(
            "INSERT INTO registry_links(link_id, machine_id, worker_credential_sha256, created_at, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                link.link_id,
                machine_id,
                link.worker_credential_sha256,
                link.created_at.0,
                link.expires_at.0
            ],
        )?;
        let display_name: String = transaction.query_row(
            "SELECT display_name FROM registry_machines WHERE machine_id = ?1",
            [machine_id],
            |row| row.get(0),
        )?;
        transaction.commit()?;
        Ok(Some(ExchangeOutcome {
            code_id,
            replaced_links,
            display_name,
        }))
    }

    /// Register or refresh the control Machine itself, keeping an owner rename.
    pub fn upsert_control_machine(&self, machine: &RegistryRow) -> Result<(), StoreError> {
        self.lock()?.execute(
            "INSERT INTO registry_machines(machine_id, display_name, role, os, daemon_version,
                 capabilities_json, endpoint_json, state, paired_at, last_seen, last_sequence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?8, 0)
             ON CONFLICT(machine_id) DO UPDATE SET
                 os = excluded.os, daemon_version = excluded.daemon_version,
                 capabilities_json = excluded.capabilities_json, last_seen = excluded.last_seen",
            params![
                machine.machine_id.0,
                machine.display_name,
                role_text(machine.role),
                machine
                    .os
                    .map(|os| serde_json::to_string(&os))
                    .transpose()?,
                machine.daemon_version,
                serde_json::to_string(&machine.capabilities)?,
                RegistryState::Active.as_str(),
                machine.paired_at.0,
            ],
        )?;
        Ok(())
    }

    /// Every registry entry, control first then by pairing time.
    pub fn registry_machines(&self) -> Result<Vec<RegistryRow>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "{REGISTRY_SELECT} ORDER BY role = 'CONTROL' DESC, paired_at, machine_id"
        ))?;
        let rows = statement.query_map([], raw_registry_row)?;
        rows.map(|row| registry_row(row?)).collect()
    }

    /// One registry entry.
    pub fn registry_machine(
        &self,
        machine_id: &MachineId,
    ) -> Result<Option<RegistryRow>, StoreError> {
        self.lock()?
            .query_row(
                &format!("{REGISTRY_SELECT} WHERE machine_id = ?1"),
                [&machine_id.0],
                raw_registry_row,
            )
            .optional()?
            .map(registry_row)
            .transpose()
    }

    /// Rename a Machine; `false` when it is unknown.
    pub fn rename_registry_machine(
        &self,
        machine_id: &MachineId,
        display_name: &str,
    ) -> Result<bool, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE registry_machines SET display_name = ?2 WHERE machine_id = ?1",
            params![machine_id.0, display_name],
        )? == 1)
    }

    /// Revoke a worker and every link; `None` when no worker has this ID.
    pub fn revoke_registry_machine(
        &self,
        machine_id: &MachineId,
        now: Timestamp,
    ) -> Result<Option<Vec<String>>, StoreError> {
        self.end_machine(machine_id, now, RegistryState::Revoked)
    }

    /// Mark a worker disconnected and revoke its links.
    pub fn disconnect_registry_machine(
        &self,
        machine_id: &MachineId,
        now: Timestamp,
    ) -> Result<Option<Vec<String>>, StoreError> {
        self.end_machine(machine_id, now, RegistryState::Disconnected)
    }

    fn end_machine(
        &self,
        machine_id: &MachineId,
        now: Timestamp,
        state: RegistryState,
    ) -> Result<Option<Vec<String>>, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let updated = transaction.execute(
            "UPDATE registry_machines SET state = ?2 WHERE machine_id = ?1 AND role = 'WORKER'",
            params![machine_id.0, state.as_str()],
        )?;
        if updated == 0 {
            return Ok(None);
        }
        let links = {
            let mut statement = transaction.prepare(
                "UPDATE registry_links SET revoked_at = ?2
                 WHERE machine_id = ?1 AND revoked_at IS NULL RETURNING link_id",
            )?;
            let rows = statement.query_map(params![machine_id.0, now.0], |row| row.get(0))?;
            rows.collect::<Result<Vec<String>, _>>()?
        };
        transaction.commit()?;
        Ok(Some(links))
    }

    /// The unrevoked, unexpired link whose current (or in-grace previous) hash matches.
    pub fn active_link_by_hash(
        &self,
        sha256: &str,
        now: Timestamp,
    ) -> Result<Option<LinkRow>, StoreError> {
        Ok(self
            .lock()?
            .query_row(
                &format!(
                    "{LINK_SELECT} WHERE revoked_at IS NULL AND expires_at > ?2
                     AND (worker_credential_sha256 = ?1
                          OR (previous_sha256 = ?1 AND previous_valid_until > ?2))"
                ),
                params![sha256, now.0],
                link_row,
            )
            .optional()?)
    }

    /// Any link (revoked or not) whose current or previous hash matches.
    pub fn link_by_hash(&self, sha256: &str) -> Result<Option<LinkRow>, StoreError> {
        Ok(self
            .lock()?
            .query_row(
                &format!(
                    "{LINK_SELECT} WHERE worker_credential_sha256 = ?1 OR previous_sha256 = ?1
                     ORDER BY revoked_at IS NULL DESC LIMIT 1"
                ),
                [sha256],
                link_row,
            )
            .optional()?)
    }

    /// The current unrevoked link of a Machine.
    pub fn active_link_for(&self, machine_id: &MachineId) -> Result<Option<LinkRow>, StoreError> {
        Ok(self
            .lock()?
            .query_row(
                &format!("{LINK_SELECT} WHERE machine_id = ?1 AND revoked_at IS NULL"),
                [&machine_id.0],
                link_row,
            )
            .optional()?)
    }

    /// Accept a heartbeat only when `sequence` exceeds the last one.
    #[allow(clippy::too_many_arguments)]
    pub fn record_heartbeat(
        &self,
        machine_id: &MachineId,
        link_id: &str,
        sequence: u64,
        at: Timestamp,
        endpoint: &LinkEndpoint,
        daemon_version: &str,
        capabilities: &[MachineCapability],
    ) -> Result<bool, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let sequence = to_sql_id(sequence)?;
        let accepted = transaction.execute(
            "UPDATE registry_machines SET last_sequence = ?2, last_seen = ?3, endpoint_json = ?4,
                 daemon_version = ?5, capabilities_json = ?6
             WHERE machine_id = ?1 AND state = 'ACTIVE' AND last_sequence < ?2",
            params![
                machine_id.0,
                sequence,
                at.0,
                serde_json::to_string(endpoint)?,
                daemon_version,
                serde_json::to_string(capabilities)?,
            ],
        )? == 1;
        if accepted {
            transaction.execute(
                "UPDATE registry_links SET last_used_at = ?2 WHERE link_id = ?1",
                params![link_id, at.0],
            )?;
        }
        transaction.commit()?;
        Ok(accepted)
    }

    /// Replace a link's credential hash, keeping the old one until `grace_until`.
    pub fn rotate_link(
        &self,
        link_id: &str,
        new_sha256: &str,
        expires_at: Timestamp,
        grace_until: Timestamp,
        now: Timestamp,
    ) -> Result<bool, StoreError> {
        Ok(self.lock()?.execute(
            "UPDATE registry_links SET previous_sha256 = worker_credential_sha256,
                 previous_valid_until = ?4, worker_credential_sha256 = ?2,
                 created_at = ?5, expires_at = ?3
             WHERE link_id = ?1 AND revoked_at IS NULL",
            params![link_id, new_sha256, expires_at.0, grace_until.0, now.0],
        )? == 1)
    }

    /// Rewrite a pre-4.0 hostname Machine ID to the immutable ID; idempotent.
    pub fn rewrite_machine_id(
        &self,
        legacy: &str,
        machine_id: &MachineId,
    ) -> Result<usize, StoreError> {
        if legacy == machine_id.0 {
            return Ok(0);
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let sessions = transaction.execute(
            "UPDATE sessions SET json = json_set(json, '$.machine_id', ?2)
             WHERE json_extract(json, '$.machine_id') = ?1",
            params![legacy, machine_id.0],
        )?;
        let commands = transaction.execute(
            "UPDATE commands SET machine_id = ?2 WHERE machine_id = ?1",
            params![legacy, machine_id.0],
        )?;
        transaction.execute("DELETE FROM machines WHERE id = ?1", [legacy])?;
        transaction.commit()?;
        Ok(sessions + commands)
    }
}

const REGISTRY_SELECT: &str = "SELECT machine_id, display_name, role, os, daemon_version,
    capabilities_json, endpoint_json, state, paired_at, last_seen, last_sequence FROM registry_machines";
const LINK_SELECT: &str =
    "SELECT link_id, machine_id, worker_credential_sha256, created_at, expires_at,
    previous_sha256, previous_valid_until, revoked_at, last_used_at FROM registry_links";

type RawRegistryRow = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
    String,
    i64,
    Option<i64>,
    i64,
);

fn raw_registry_row(row: &Row<'_>) -> rusqlite::Result<RawRegistryRow> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
        row.get(9)?,
        row.get(10)?,
    ))
}

fn registry_row(raw: RawRegistryRow) -> Result<RegistryRow, StoreError> {
    let (id, name, role, os, version, capabilities, endpoint, state, paired, seen, sequence) = raw;
    Ok(RegistryRow {
        machine_id: MachineId(id),
        display_name: name,
        role: match role.as_str() {
            "CONTROL" => MachineRole::Control,
            "WORKER" => MachineRole::Worker,
            other => {
                return Err(StoreError::InvalidRegistryValue {
                    field: "role",
                    value: other.to_string(),
                })
            }
        },
        os: os.map(|os| serde_json::from_str(&os)).transpose()?,
        daemon_version: version,
        capabilities: serde_json::from_str(&capabilities)?,
        endpoint: endpoint
            .map(|endpoint| serde_json::from_str(&endpoint))
            .transpose()?,
        state: RegistryState::parse(&state)?,
        paired_at: Timestamp(paired),
        last_seen: seen.map(Timestamp),
        last_sequence: u64::try_from(sequence)
            .map_err(|_| StoreError::InvalidStoredId { value: sequence })?,
    })
}

fn link_row(row: &Row<'_>) -> rusqlite::Result<LinkRow> {
    Ok(LinkRow {
        link_id: row.get(0)?,
        machine_id: MachineId(row.get(1)?),
        worker_credential_sha256: row.get(2)?,
        created_at: Timestamp(row.get(3)?),
        expires_at: Timestamp(row.get(4)?),
        previous_sha256: row.get(5)?,
        previous_valid_until: row.get::<_, Option<i64>>(6)?.map(Timestamp),
        revoked_at: row.get::<_, Option<i64>>(7)?.map(Timestamp),
        last_used_at: row.get::<_, Option<i64>>(8)?.map(Timestamp),
    })
}

fn code_row(row: &Row<'_>) -> rusqlite::Result<PairingCodeRow> {
    Ok(PairingCodeRow {
        code_id: row.get(0)?,
        code_hmac: row.get(1)?,
        created_at: Timestamp(row.get(2)?),
        expires_at: Timestamp(row.get(3)?),
        consumed_at: row.get::<_, Option<i64>>(4)?.map(Timestamp),
        cancelled_at: row.get::<_, Option<i64>>(5)?.map(Timestamp),
        machine_id: row.get::<_, Option<String>>(6)?.map(MachineId),
    })
}

fn role_text(role: MachineRole) -> &'static str {
    match role {
        MachineRole::Control => "CONTROL",
        MachineRole::Worker => "WORKER",
    }
}

fn count(store: &Store, sql: &str, at: Timestamp) -> Result<u32, StoreError> {
    let value: i64 = store.lock()?.query_row(sql, [at.0], |row| row.get(0))?;
    Ok(u32::try_from(value).unwrap_or(u32::MAX))
}
