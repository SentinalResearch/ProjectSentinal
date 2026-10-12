//! Durable worker projection outbox and control-side materialized rows.

use crate::{to_sql_id, Store, StoreError};
use protocol::{
    MachineId, ProjectedEventSummary, ProjectedMetrics, ProjectedSession,
    ProjectionAcknowledgement, ProjectionAcknowledgementStatus, ProjectionBatch, ProjectionChange,
    ProjectionResyncReason, ProjectionSyncReport, Timestamp,
};
use rusqlite::{params, OptionalExtension, Transaction};

/// Default maximum pending projection rows.
pub const DEFAULT_PROJECTION_OUTBOX_ROWS: u32 = 20_000;
/// Default maximum encoded projection bytes.
pub const DEFAULT_PROJECTION_OUTBOX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PROJECTION_SNAPSHOT_CHUNKS: u32 = 16;

/// Bounded worker projection outbox limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProjectionOutboxLimits {
    /// Maximum pending rows.
    pub maximum_rows: u32,
    /// Maximum encoded bytes.
    pub maximum_bytes: u64,
}

/// Cursor and timestamps for one control-side Machine projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlProjectionState {
    /// Worker source epoch.
    pub source_epoch: String,
    /// Latest committed source cursor.
    pub committed_cursor: u64,
    /// Newest worker observation represented.
    pub observed_at: Option<Timestamp>,
    /// Control commit time.
    pub synced_at: Timestamp,
    /// Latest control-view revision for this Machine.
    pub view_revision: u64,
}

/// One persisted control-view change and its monotonic revision.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlProjectionRevision {
    /// Control-wide revision.
    pub revision: u64,
    /// Machine whose projection changed.
    pub machine_id: MachineId,
    /// Privacy-minimized projection mutation.
    pub change: ProjectionChange,
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
        if let Some((existing_cursor, replacement)) =
            compactable_change(&transaction, machine_id, source_epoch, change)?
        {
            let replacement_json = serde_json::to_string(&replacement)?;
            let replacement_bytes = replacement_json.len() as u64;
            let previous_bytes: u64 = transaction.query_row(
                "SELECT encoded_bytes FROM projection_outbox
                 WHERE machine_id=?1 AND source_epoch=?2 AND cursor=?3",
                params![machine_id.0, source_epoch, to_sql_id(existing_cursor)?],
                |row| row.get(0),
            )?;
            let pending_bytes: u64 = transaction.query_row(
                "SELECT COALESCE(SUM(encoded_bytes),0) FROM projection_outbox
                 WHERE machine_id=?1 AND source_epoch=?2",
                params![machine_id.0, source_epoch],
                |row| row.get(0),
            )?;
            let compacted_bytes = pending_bytes
                .saturating_sub(previous_bytes)
                .saturating_add(replacement_bytes);
            if compacted_bytes > limits.maximum_bytes {
                return Err(StoreError::ProjectionOutboxFull {
                    rows: transaction.query_row(
                        "SELECT COUNT(*) FROM projection_outbox
                         WHERE machine_id=?1 AND source_epoch=?2",
                        params![machine_id.0, source_epoch],
                        |row| row.get(0),
                    )?,
                    bytes: pending_bytes,
                    maximum_rows: limits.maximum_rows,
                    maximum_bytes: limits.maximum_bytes,
                });
            }
            transaction.execute(
                "UPDATE projection_outbox SET change_json=?1,encoded_bytes=?2,created_at=?3
                 WHERE machine_id=?4 AND source_epoch=?5 AND cursor=?6",
                params![
                    replacement_json,
                    replacement_bytes,
                    at.0,
                    machine_id.0,
                    source_epoch,
                    to_sql_id(existing_cursor)?
                ],
            )?;
            transaction.execute(
                "UPDATE projection_worker_state SET updated_at=?1 WHERE machine_id=?2",
                params![at.0, machine_id.0],
            )?;
            update_worker_projection_record(
                &transaction,
                machine_id,
                source_epoch,
                &replacement,
                at,
            )?;
            transaction.commit()?;
            return Ok(());
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
            transaction.execute(
                "DELETE FROM worker_projection_records WHERE machine_id=?1",
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
        update_worker_projection_record(&transaction, machine_id, source_epoch, change, at)?;
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

    /// Return the worker's bounded current projection for a full resynchronization.
    pub fn worker_projection_snapshot(
        &self,
        machine_id: &MachineId,
        source_epoch: &str,
    ) -> Result<Vec<ProjectionChange>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT change_json FROM worker_projection_records
             WHERE machine_id=?1 AND source_epoch=?2
             ORDER BY source_cursor,record_key",
        )?;
        let values = statement
            .query_map(params![machine_id.0, source_epoch], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        values
            .into_iter()
            .map(|json| serde_json::from_str(&json).map_err(StoreError::from))
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
        if let ProjectionBatch::Snapshot { machine_id, .. } = batch {
            if machine_id != authenticated_machine {
                return Err(StoreError::ProjectionMachineMismatch {
                    authenticated: authenticated_machine.0.clone(),
                    claimed: machine_id.0.clone(),
                });
            }
            return self.apply_projection_snapshot(authenticated_machine, batch, at);
        }
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

    fn apply_projection_snapshot(
        &self,
        machine_id: &MachineId,
        batch: &ProjectionBatch,
        at: Timestamp,
    ) -> Result<ProjectionAcknowledgement, StoreError> {
        let ProjectionBatch::Snapshot {
            source_epoch,
            snapshot_id,
            snapshot_cursor,
            chunk_index,
            final_chunk,
            sent_at,
            records,
            ..
        } = batch
        else {
            return Err(StoreError::MissingText {
                kind: "projection snapshot",
                id: machine_id.0.clone(),
            });
        };
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let stored = control_state(&transaction, machine_id)?;
        let committed = stored.as_ref().map_or(0, |(_, cursor)| *cursor);
        if stored
            .as_ref()
            .is_some_and(|(epoch, cursor)| epoch == source_epoch && cursor == snapshot_cursor)
        {
            return Ok(ProjectionAcknowledgement {
                status: ProjectionAcknowledgementStatus::Duplicate,
                source_epoch: source_epoch.clone(),
                committed_cursor: committed,
                reason: None,
            });
        }
        if *chunk_index >= MAX_PROJECTION_SNAPSHOT_CHUNKS {
            return Ok(resync(
                source_epoch,
                committed,
                ProjectionResyncReason::CursorMismatch,
            ));
        }
        if *chunk_index == 0 {
            transaction.execute(
                "DELETE FROM control_projection_snapshot_chunks WHERE machine_id=?1",
                [&machine_id.0],
            )?;
        } else {
            let previous = transaction
                .query_row(
                    "SELECT chunk_index,snapshot_cursor FROM control_projection_snapshot_chunks
                     WHERE machine_id=?1 AND source_epoch=?2 AND snapshot_id=?3
                     ORDER BY chunk_index DESC LIMIT 1",
                    params![machine_id.0, source_epoch, snapshot_id],
                    |row| Ok((row.get::<_, u32>(0)?, row.get::<_, u64>(1)?)),
                )
                .optional()?;
            if previous != Some((chunk_index.saturating_sub(1), *snapshot_cursor)) {
                return Ok(resync(
                    source_epoch,
                    committed,
                    ProjectionResyncReason::CursorMismatch,
                ));
            }
        }
        transaction.execute(
            "INSERT OR REPLACE INTO control_projection_snapshot_chunks(
                machine_id,source_epoch,snapshot_id,chunk_index,snapshot_cursor,
                records_json,sent_at,created_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                machine_id.0,
                source_epoch,
                snapshot_id,
                chunk_index,
                to_sql_id(*snapshot_cursor)?,
                serde_json::to_string(records)?,
                sent_at.0,
                at.0
            ],
        )?;
        if !final_chunk {
            transaction.commit()?;
            return Ok(ProjectionAcknowledgement {
                status: ProjectionAcknowledgementStatus::Accepted,
                source_epoch: source_epoch.clone(),
                committed_cursor: committed,
                reason: None,
            });
        }
        let staged = {
            let mut statement = transaction.prepare(
                "SELECT records_json FROM control_projection_snapshot_chunks
                 WHERE machine_id=?1 AND source_epoch=?2 AND snapshot_id=?3
                 ORDER BY chunk_index",
            )?;
            let rows = statement
                .query_map(params![machine_id.0, source_epoch, snapshot_id], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        clear_control_projection(&transaction, machine_id)?;
        for encoded in staged {
            let changes: Vec<ProjectionChange> = serde_json::from_str(&encoded)?;
            for change in &changes {
                apply_change(&transaction, machine_id, change, at)?;
            }
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
                machine_id.0,
                source_epoch,
                protocol::PROJECTION_SCHEMA_VERSION,
                to_sql_id(*snapshot_cursor)?,
                sent_at.0,
                at.0,
                to_sql_id(revision)?
            ],
        )?;
        transaction.execute(
            "DELETE FROM control_projection_snapshot_chunks WHERE machine_id=?1",
            [&machine_id.0],
        )?;
        transaction.commit()?;
        Ok(ProjectionAcknowledgement {
            status: ProjectionAcknowledgementStatus::Accepted,
            source_epoch: source_epoch.clone(),
            committed_cursor: *snapshot_cursor,
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

    /// Return bounded newest safe Event summaries for one Machine.
    pub fn projected_events(
        &self,
        machine_id: &MachineId,
        limit: u32,
    ) -> Result<Vec<ProjectedEventSummary>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT event_json FROM control_projection_events
             WHERE machine_id=?1 ORDER BY at DESC,event_id DESC LIMIT ?2",
        )?;
        let values = statement
            .query_map(params![machine_id.0, limit], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        values
            .iter()
            .map(|json| serde_json::from_str(json).map_err(StoreError::from))
            .collect()
    }

    /// Return coalesced latest Metrics for one Machine.
    pub fn projected_metrics(
        &self,
        machine_id: &MachineId,
        limit: u32,
    ) -> Result<Vec<ProjectedMetrics>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT metrics_json FROM control_projection_metrics
             WHERE machine_id=?1 ORDER BY at DESC,scope,session_key LIMIT ?2",
        )?;
        let values = statement
            .query_map(params![machine_id.0, limit], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        values
            .iter()
            .map(|json| serde_json::from_str(json).map_err(StoreError::from))
            .collect()
    }

    /// Return control cursor and freshness timestamps for one Machine.
    pub fn control_projection_state(
        &self,
        machine_id: &MachineId,
    ) -> Result<Option<ControlProjectionState>, StoreError> {
        Ok(self
            .lock()?
            .query_row(
                "SELECT source_epoch,committed_cursor,observed_at,synced_at,view_revision
                 FROM control_projection_state WHERE machine_id=?1",
                [&machine_id.0],
                |row| {
                    Ok(ControlProjectionState {
                        source_epoch: row.get(0)?,
                        committed_cursor: row.get(1)?,
                        observed_at: row.get::<_, Option<i64>>(2)?.map(Timestamp),
                        synced_at: Timestamp(row.get(3)?),
                        view_revision: row.get(4)?,
                    })
                },
            )
            .optional()?)
    }

    /// Latest control-view revision across all Machines.
    pub fn control_projection_revision(&self) -> Result<u64, StoreError> {
        Ok(self.lock()?.query_row(
            "SELECT COALESCE(MAX(revision),0) FROM control_projection_changes",
            [],
            |row| row.get(0),
        )?)
    }

    /// Return bounded committed changes after one control-view revision.
    pub fn control_projection_changes_after(
        &self,
        after: u64,
        limit: u32,
    ) -> Result<Vec<ControlProjectionRevision>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT revision,machine_id,change_json FROM control_projection_changes
             WHERE revision>?1 ORDER BY revision LIMIT ?2",
        )?;
        let rows = statement
            .query_map(params![to_sql_id(after)?, limit], |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|(revision, machine_id, json)| {
                Ok(ControlProjectionRevision {
                    revision,
                    machine_id: MachineId(machine_id),
                    change: serde_json::from_str(&json)?,
                })
            })
            .collect()
    }

    /// Remove only one revoked Machine's disposable control projection.
    pub fn purge_control_projection(&self, machine_id: &MachineId) -> Result<(), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        clear_control_projection(&transaction, machine_id)?;
        transaction.execute(
            "DELETE FROM control_projection_changes WHERE machine_id=?1",
            [&machine_id.0],
        )?;
        transaction.execute(
            "DELETE FROM control_projection_state WHERE machine_id=?1",
            [&machine_id.0],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

fn update_worker_projection_record(
    transaction: &Transaction<'_>,
    machine_id: &MachineId,
    source_epoch: &str,
    change: &ProjectionChange,
    at: Timestamp,
) -> Result<(), StoreError> {
    if let ProjectionChange::SessionTombstone { session_id, .. } = change {
        transaction.execute(
            "DELETE FROM worker_projection_records
             WHERE machine_id=?1 AND source_epoch=?2 AND
             (record_key=?3 OR record_key=?4 OR record_key LIKE ?5)",
            params![
                machine_id.0,
                source_epoch,
                format!("session:{}", session_id.0),
                format!("metrics:session:{}", session_id.0),
                format!("event:{}:%", session_id.0)
            ],
        )?;
        return Ok(());
    }
    let key = worker_record_key(change);
    transaction.execute(
        "INSERT INTO worker_projection_records(
            machine_id,source_epoch,record_key,source_cursor,change_json,observed_at
         ) VALUES (?1,?2,?3,?4,?5,?6)
         ON CONFLICT(machine_id,source_epoch,record_key) DO UPDATE SET
            source_cursor=excluded.source_cursor,
            change_json=excluded.change_json,
            observed_at=excluded.observed_at",
        params![
            machine_id.0,
            source_epoch,
            key,
            to_sql_id(change.cursor())?,
            serde_json::to_string(change)?,
            at.0
        ],
    )?;
    if let ProjectionChange::EventAppend { event, .. } = change {
        transaction.execute(
            "DELETE FROM worker_projection_records
             WHERE machine_id=?1 AND source_epoch=?2 AND record_key LIKE ?3
               AND record_key NOT IN (
                 SELECT record_key FROM worker_projection_records
                 WHERE machine_id=?1 AND source_epoch=?2 AND record_key LIKE ?3
                 ORDER BY observed_at DESC,source_cursor DESC LIMIT 32
               )",
            params![
                machine_id.0,
                source_epoch,
                format!("event:{}:%", event.session_id.0)
            ],
        )?;
    }
    Ok(())
}

fn worker_record_key(change: &ProjectionChange) -> String {
    match change {
        ProjectionChange::SessionUpsert { session, .. } => format!("session:{}", session.id.0),
        ProjectionChange::SessionTombstone { session_id, .. } => {
            format!("session:{}", session_id.0)
        }
        ProjectionChange::EventAppend { event, .. } => {
            format!("event:{}:{}", event.session_id.0, event.id.0)
        }
        ProjectionChange::MetricsReplace { metrics, .. } => metrics.session_id.map_or_else(
            || "metrics:machine".into(),
            |session_id| format!("metrics:session:{}", session_id.0),
        ),
    }
}

fn compactable_change(
    transaction: &Transaction<'_>,
    machine_id: &MachineId,
    source_epoch: &str,
    incoming: &ProjectionChange,
) -> Result<Option<(u64, ProjectionChange)>, StoreError> {
    let mut statement = transaction.prepare(
        "SELECT cursor,change_json FROM projection_outbox
         WHERE machine_id=?1 AND source_epoch=?2 ORDER BY cursor",
    )?;
    let rows = statement
        .query_map(params![machine_id.0, source_epoch], |row| {
            Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    for (cursor, json) in rows.into_iter().rev() {
        let existing: ProjectionChange = serde_json::from_str(&json)?;
        if changes_share_compaction_key(&existing, incoming) {
            return Ok(Some((cursor, change_with_cursor(incoming, cursor))));
        }
    }
    Ok(None)
}

fn changes_share_compaction_key(existing: &ProjectionChange, incoming: &ProjectionChange) -> bool {
    match (existing, incoming) {
        (
            ProjectionChange::MetricsReplace { metrics: left, .. },
            ProjectionChange::MetricsReplace { metrics: right, .. },
        ) => left.scope == right.scope && left.session_id == right.session_id,
        (
            ProjectionChange::SessionUpsert { session: left, .. },
            ProjectionChange::SessionUpsert { session: right, .. },
        ) => left.id == right.id && !left.state.is_finished() && !right.state.is_finished(),
        _ => false,
    }
}

fn change_with_cursor(change: &ProjectionChange, cursor: u64) -> ProjectionChange {
    match change {
        ProjectionChange::SessionUpsert { session, .. } => ProjectionChange::SessionUpsert {
            cursor,
            session: session.clone(),
        },
        ProjectionChange::SessionTombstone {
            session_id,
            removed_at,
            ..
        } => ProjectionChange::SessionTombstone {
            cursor,
            session_id: *session_id,
            removed_at: *removed_at,
        },
        ProjectionChange::EventAppend { event, .. } => ProjectionChange::EventAppend {
            cursor,
            event: event.clone(),
        },
        ProjectionChange::MetricsReplace { metrics, .. } => ProjectionChange::MetricsReplace {
            cursor,
            metrics: metrics.clone(),
        },
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
