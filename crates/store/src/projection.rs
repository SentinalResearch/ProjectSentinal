//! Durable worker projection outbox and control-side materialized rows.

use crate::{to_sql_id, Store, StoreError};
use protocol::{
    MachineId, ProjectedSession, ProjectionAcknowledgement, ProjectionAcknowledgementStatus,
    ProjectionBatch, ProjectionChange, ProjectionResyncReason, ProjectionSyncReport, Timestamp,
};
use rusqlite::{params, OptionalExtension, Transaction};

/// Default maximum pending projection rows.
pub const DEFAULT_PROJECTION_OUTBOX_ROWS: u32 = 20_000;
/// Default maximum encoded projection bytes.
pub const DEFAULT_PROJECTION_OUTBOX_BYTES: u64 = 16 * 1024 * 1024;

/// Bounded worker projection outbox limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionOutboxLimits {
    /// Maximum pending rows.
    pub maximum_rows: u32,
    /// Maximum encoded bytes.
    pub maximum_bytes: u64,
}

impl Default for ProjectionOutboxLimits {
    fn default() -> Self {
        Self {
            maximum_rows: DEFAULT_PROJECTION_OUTBOX_ROWS,
            maximum_bytes: DEFAULT_PROJECTION_OUTBOX_BYTES,
        }
    }
}

impl Store {
    /// Append one strictly ordered projection change within durable bounds.
    pub fn record_projection_change(
        &self,
        machine_id: &MachineId,
        source_epoch: &str,
        change: &ProjectionChange,
        at: Timestamp,
        limits: ProjectionOutboxLimits,
    ) -> Result<(), StoreError> {
        let json = serde_json::to_string(change)?;
        let bytes = json.len() as u64;
        let cursor = change.cursor();
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let state = worker_state(&transaction, machine_id)?;
        let current = match &state {
            Some((epoch, current, _)) if epoch == source_epoch => *current,
            Some(_) => 0,
            None => 0,
        };
        let expected = current.saturating_add(1);
        if cursor != expected {
            return Err(StoreError::ProjectionCursor {
                machine_id: machine_id.0.clone(),
                expected,
                got: cursor,
            });
        }
        let (rows, pending_bytes): (u32, u64) = transaction.query_row(
            "SELECT COUNT(*),COALESCE(SUM(encoded_bytes),0) FROM projection_outbox
             WHERE machine_id=?1 AND source_epoch=?2",
            params![machine_id.0, source_epoch],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if rows >= limits.maximum_rows || pending_bytes.saturating_add(bytes) > limits.maximum_bytes
        {
            return Err(StoreError::ProjectionOutboxFull {
                rows,
                bytes: pending_bytes,
                maximum_rows: limits.maximum_rows,
                maximum_bytes: limits.maximum_bytes,
            });
        }
        if state
            .as_ref()
            .is_some_and(|(epoch, _, _)| epoch != source_epoch)
        {
            transaction.execute(
                "DELETE FROM projection_outbox WHERE machine_id=?1",
                [&machine_id.0],
            )?;
        }
        transaction.execute(
            "INSERT INTO projection_worker_state(
                machine_id,source_epoch,current_cursor,acknowledged_cursor,updated_at
             ) VALUES (?1,?2,?3,0,?4)
             ON CONFLICT(machine_id) DO UPDATE SET
                source_epoch=excluded.source_epoch,
                current_cursor=excluded.current_cursor,
                acknowledged_cursor=CASE
                    WHEN projection_worker_state.source_epoch=excluded.source_epoch
                    THEN projection_worker_state.acknowledged_cursor ELSE 0 END,
                updated_at=excluded.updated_at",
            params![machine_id.0, source_epoch, to_sql_id(cursor)?, at.0],
        )?;
        transaction.execute(
            "INSERT INTO projection_outbox(
                machine_id,source_epoch,cursor,change_json,encoded_bytes,created_at
             ) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                machine_id.0,
                source_epoch,
                to_sql_id(cursor)?,
                json,
                bytes,
                at.0
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Return worker cursor state for heartbeat negotiation.
    pub fn projection_sync_report(
        &self,
        machine_id: &MachineId,
        source_epoch: &str,
    ) -> Result<ProjectionSyncReport, StoreError> {
        let connection = self.lock()?;
        let state = worker_state(&connection, machine_id)?;
        let (current_cursor, last_acked_cursor) = state
            .filter(|(epoch, _, _)| epoch == source_epoch)
            .map(|(_, current, acknowledged)| (current, acknowledged))
            .unwrap_or((0, 0));
        Ok(ProjectionSyncReport {
            schema_version: protocol::PROJECTION_SCHEMA_VERSION,
            source_epoch: source_epoch.into(),
            current_cursor,
            last_acked_cursor,
        })
    }

    /// Return ordered pending changes after a source cursor.
    pub fn projection_changes_after(
        &self,
        machine_id: &MachineId,
        source_epoch: &str,
        after: u64,
        limit: u32,
    ) -> Result<Vec<ProjectionChange>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT change_json FROM projection_outbox
             WHERE machine_id=?1 AND source_epoch=?2 AND cursor>?3
             ORDER BY cursor LIMIT ?4",
        )?;
        let json = statement
            .query_map(
                params![machine_id.0, source_epoch, to_sql_id(after)?, limit],
                |row| row.get::<_, String>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        json.iter()
            .map(|value| serde_json::from_str(value).map_err(StoreError::from))
            .collect()
    }

    /// Remove changes through a durably acknowledged cursor.
    pub fn acknowledge_projection_cursor(
        &self,
        machine_id: &MachineId,
        source_epoch: &str,
        cursor: u64,
    ) -> Result<usize, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let state = worker_state(&transaction, machine_id)?;
        let Some((epoch, current, acknowledged)) = state else {
            return Err(StoreError::MissingText {
                kind: "projection worker state",
                id: machine_id.0.clone(),
            });
        };
        if epoch != source_epoch || cursor > current || cursor < acknowledged {
            return Err(StoreError::ProjectionCursor {
                machine_id: machine_id.0.clone(),
                expected: acknowledged,
                got: cursor,
            });
        }
        let removed = transaction.execute(
            "DELETE FROM projection_outbox
             WHERE machine_id=?1 AND source_epoch=?2 AND cursor<=?3",
            params![machine_id.0, source_epoch, to_sql_id(cursor)?],
        )?;
        transaction.execute(
            "UPDATE projection_worker_state SET acknowledged_cursor=?1
             WHERE machine_id=?2 AND source_epoch=?3",
            params![to_sql_id(cursor)?, machine_id.0, source_epoch],
        )?;
        transaction.commit()?;
        Ok(removed)
    }

    /// Atomically apply an authenticated worker delta or bounded snapshot.
    pub fn apply_projection_batch(
        &self,
        authenticated_machine: &MachineId,
        batch: &ProjectionBatch,
        at: Timestamp,
    ) -> Result<ProjectionAcknowledgement, StoreError> {
        batch.validate()?;
        let (claimed, epoch, base, through, changes, replace) = batch_parts(batch);
        if claimed != authenticated_machine {
            return Err(StoreError::ProjectionMachineMismatch {
                authenticated: authenticated_machine.0.clone(),
                claimed: claimed.0.clone(),
            });
        }
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let stored = control_state(&transaction, authenticated_machine)?;
        if let Some((stored_epoch, committed)) = &stored {
            if stored_epoch != epoch {
                return Ok(resync(
                    epoch,
                    *committed,
                    ProjectionResyncReason::SourceEpochChanged,
                ));
            }
            if !replace && through <= *committed {
                return Ok(ProjectionAcknowledgement {
                    status: ProjectionAcknowledgementStatus::Duplicate,
                    source_epoch: epoch.into(),
                    committed_cursor: *committed,
                    reason: None,
                });
            }
            if !replace && base != *committed {
                return Ok(resync(
                    epoch,
                    *committed,
                    ProjectionResyncReason::CursorMismatch,
                ));
            }
        } else if !replace && base != 0 {
            return Ok(resync(epoch, 0, ProjectionResyncReason::CursorMismatch));
        }
        if replace {
            clear_control_projection(&transaction, authenticated_machine)?;
        }
        for change in changes {
            apply_change(&transaction, authenticated_machine, change, at)?;
        }
        let revision: u64 = transaction.query_row(
            "SELECT COALESCE(MAX(revision),0) FROM control_projection_changes",
            [],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO control_projection_state(
                machine_id,source_epoch,schema_version,committed_cursor,observed_at,synced_at,view_revision
             ) VALUES (?1,?2,?3,?4,?5,?6,?7)
             ON CONFLICT(machine_id) DO UPDATE SET
                source_epoch=excluded.source_epoch,
                schema_version=excluded.schema_version,
                committed_cursor=excluded.committed_cursor,
                observed_at=excluded.observed_at,
                synced_at=excluded.synced_at,
                view_revision=excluded.view_revision",
            params![
                authenticated_machine.0,
                epoch,
                protocol::PROJECTION_SCHEMA_VERSION,
                to_sql_id(through)?,
                batch_sent_at(batch).0,
                at.0,
                to_sql_id(revision)?
            ],
        )?;
        transaction.commit()?;
        Ok(ProjectionAcknowledgement {
            status: ProjectionAcknowledgementStatus::Accepted,
            source_epoch: epoch.into(),
            committed_cursor: through,
            reason: None,
        })
    }

    /// Return bounded Session summaries for one Machine.
    pub fn projected_sessions(
        &self,
        machine_id: &MachineId,
        limit: u32,
    ) -> Result<Vec<ProjectedSession>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT session_json FROM control_projection_sessions
             WHERE machine_id=?1 ORDER BY updated_at DESC,session_id DESC LIMIT ?2",
        )?;
        let values = statement
            .query_map(params![machine_id.0, limit], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        values
            .iter()
            .map(|json| serde_json::from_str(json).map_err(StoreError::from))
            .collect()
    }
}

fn worker_state(
    connection: &rusqlite::Connection,
    machine_id: &MachineId,
) -> Result<Option<(String, u64, u64)>, StoreError> {
    Ok(connection
        .query_row(
            "SELECT source_epoch,current_cursor,acknowledged_cursor
             FROM projection_worker_state WHERE machine_id=?1",
            [&machine_id.0],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?)
}

fn control_state(
    transaction: &Transaction<'_>,
    machine_id: &MachineId,
) -> Result<Option<(String, u64)>, StoreError> {
    Ok(transaction
        .query_row(
            "SELECT source_epoch,committed_cursor FROM control_projection_state
             WHERE machine_id=?1",
            [&machine_id.0],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?)
}

fn batch_parts(batch: &ProjectionBatch) -> (&MachineId, &str, u64, u64, &[ProjectionChange], bool) {
    match batch {
        ProjectionBatch::Delta {
            machine_id,
            source_epoch,
            base_cursor,
            through_cursor,
            changes,
            ..
        } => (
            machine_id,
            source_epoch,
            *base_cursor,
            *through_cursor,
            changes,
            false,
        ),
        ProjectionBatch::Snapshot {
            machine_id,
            source_epoch,
            snapshot_cursor,
            records,
            ..
        } => (machine_id, source_epoch, 0, *snapshot_cursor, records, true),
    }
}

fn batch_sent_at(batch: &ProjectionBatch) -> Timestamp {
    match batch {
        ProjectionBatch::Delta { sent_at, .. } | ProjectionBatch::Snapshot { sent_at, .. } => {
            *sent_at
        }
    }
}

fn resync(
    epoch: &str,
    committed_cursor: u64,
    reason: ProjectionResyncReason,
) -> ProjectionAcknowledgement {
    ProjectionAcknowledgement {
        status: ProjectionAcknowledgementStatus::ResyncRequired,
        source_epoch: epoch.into(),
        committed_cursor,
        reason: Some(reason),
    }
}

fn clear_control_projection(
    transaction: &Transaction<'_>,
    machine_id: &MachineId,
) -> Result<(), StoreError> {
    for table in [
        "control_projection_sessions",
        "control_projection_events",
        "control_projection_metrics",
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE machine_id=?1"),
            [&machine_id.0],
        )?;
    }
    Ok(())
}

fn apply_change(
    transaction: &Transaction<'_>,
    machine_id: &MachineId,
    change: &ProjectionChange,
    at: Timestamp,
) -> Result<(), StoreError> {
    match change {
        ProjectionChange::SessionUpsert { session, .. } => {
            transaction.execute(
                "INSERT INTO control_projection_sessions(machine_id,session_id,session_json,updated_at)
                 VALUES (?1,?2,?3,?4) ON CONFLICT(machine_id,session_id) DO UPDATE SET
                    session_json=excluded.session_json,updated_at=excluded.updated_at",
                params![
                    machine_id.0,
                    to_sql_id(session.id.0)?,
                    serde_json::to_string(session)?,
                    session.updated_at.0
                ],
            )?;
        }
        ProjectionChange::SessionTombstone { session_id, .. } => {
            transaction.execute(
                "DELETE FROM control_projection_sessions WHERE machine_id=?1 AND session_id=?2",
                params![machine_id.0, to_sql_id(session_id.0)?],
            )?;
        }
        ProjectionChange::EventAppend { event, .. } => {
            transaction.execute(
                "INSERT OR IGNORE INTO control_projection_events(
                    machine_id,event_id,session_id,at,event_json) VALUES (?1,?2,?3,?4,?5)",
                params![
                    machine_id.0,
                    to_sql_id(event.id.0)?,
                    to_sql_id(event.session_id.0)?,
                    event.at.0,
                    serde_json::to_string(event)?
                ],
            )?;
        }
        ProjectionChange::MetricsReplace { metrics, .. } => {
            let scope = serde_json::to_string(&metrics.scope)?;
            let session_key = metrics.session_id.map_or(0, |id| id.0);
            transaction.execute(
                "INSERT INTO control_projection_metrics(machine_id,scope,session_key,metrics_json,at)
                 VALUES (?1,?2,?3,?4,?5) ON CONFLICT(machine_id,scope,session_key) DO UPDATE SET
                    metrics_json=excluded.metrics_json,at=excluded.at",
                params![
                    machine_id.0,
                    scope.trim_matches('"'),
                    to_sql_id(session_key)?,
                    serde_json::to_string(metrics)?,
                    metrics.at.0
                ],
            )?;
        }
    }
    transaction.execute(
        "INSERT INTO control_projection_changes(machine_id,change_json,committed_at)
         VALUES (?1,?2,?3)",
        params![machine_id.0, serde_json::to_string(change)?, at.0],
    )?;
    Ok(())
}
