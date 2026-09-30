//! Relational Session continuation lineage.

use crate::{decode_session, state_name, to_sql_id, Store, StoreError};
use protocol::{CaptureLevel, Session, SessionId, SessionOrigin};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::collections::HashSet;

/// Maximum lineage nodes traversed before persisted data is treated as corrupt.
pub const MAX_LINEAGE_GENERATIONS: usize = 256;

impl Store {
    /// Insert one descendant after validating its finished FULL predecessor.
    pub fn insert_continuation(
        &self,
        source_id: SessionId,
        descendant: &Session,
    ) -> Result<(), StoreError> {
        descendant.validate()?;
        if descendant.continued_from != Some(source_id) {
            return Err(StoreError::ContinuationPredecessor {
                source_id,
                descendant_id: descendant.id,
                actual: descendant.continued_from,
            });
        }

        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let source = required_session_on(&transaction, source_id)?;
        if source.capture != CaptureLevel::Full {
            return Err(StoreError::ContinuationCapture {
                session_id: source.id,
                capture: source.capture,
            });
        }
        if !source.state.is_finished() {
            return Err(StoreError::ContinuationState {
                session_id: source.id,
                state: source.state,
            });
        }
        if descendant.machine_id != source.machine_id {
            return Err(StoreError::CrossMachineLineage {
                source_id,
                source_machine: source.machine_id.0,
                descendant_id: descendant.id,
                descendant_machine: descendant.machine_id.0.clone(),
            });
        }
        if descendant.origin != SessionOrigin::Ui || descendant.capture != CaptureLevel::Full {
            return Err(StoreError::ContinuationCapture {
                session_id: descendant.id,
                capture: descendant.capture,
            });
        }
        // Validate the complete existing chain before adding an edge to it.
        ordered_lineage_on(&transaction, source_id)?;
        if let Some(successor) = direct_successor_on(&transaction, source_id)? {
            return Err(StoreError::SuccessorExists {
                source_id,
                successor_id: successor.id,
            });
        }

        transaction.execute(
            "INSERT INTO sessions(id, state, updated_at, json, continued_from) VALUES (?1, ?2, ?3, ?4, ?5)",
            (
                to_sql_id(descendant.id.0)?,
                state_name(descendant.state)?,
                descendant.updated_at.0,
                serde_json::to_string(descendant)?,
                to_sql_id(source_id.0)?,
            ),
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Return the direct predecessor of a Session.
    pub fn predecessor(&self, session_id: SessionId) -> Result<Option<Session>, StoreError> {
        let connection = self.lock()?;
        let session = required_session_on(&connection, session_id)?;
        session
            .continued_from
            .map(|predecessor| {
                session_on(&connection, predecessor)?.ok_or(StoreError::MissingLineageSession {
                    session_id,
                    missing_id: predecessor,
                })
            })
            .transpose()
    }

    /// Return the single direct successor of a Session, if one exists.
    pub fn direct_successor(&self, session_id: SessionId) -> Result<Option<Session>, StoreError> {
        let connection = self.lock()?;
        required_session_on(&connection, session_id)?;
        direct_successor_on(&connection, session_id)
    }

    /// Return every generation from root through latest for a Session.
    pub fn ordered_lineage(&self, session_id: SessionId) -> Result<Vec<Session>, StoreError> {
        let connection = self.lock()?;
        ordered_lineage_on(&connection, session_id)
    }

    /// Return the latest generation in a Session's lineage.
    pub fn latest_generation(&self, session_id: SessionId) -> Result<Session, StoreError> {
        let connection = self.lock()?;
        ordered_lineage_on(&connection, session_id)?
            .pop()
            .ok_or(StoreError::Missing {
                kind: "session",
                id: session_id.0,
            })
    }
}

pub(crate) fn continued_from_on(
    connection: &Connection,
    session_id: SessionId,
) -> Result<Option<SessionId>, StoreError> {
    let value = connection
        .query_row(
            "SELECT continued_from FROM sessions WHERE id=?1",
            [to_sql_id(session_id.0)?],
            |row| row.get::<_, Option<i64>>(0),
        )
        .optional()?
        .ok_or(StoreError::Missing {
            kind: "session",
            id: session_id.0,
        })?;
    sql_session_id(value)
}

pub(crate) fn session_on(
    connection: &Connection,
    session_id: SessionId,
) -> Result<Option<Session>, StoreError> {
    let row = connection
        .query_row(
            "SELECT json, continued_from FROM sessions WHERE id=?1",
            [to_sql_id(session_id.0)?],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()?;
    row.map(|(json, predecessor)| decode_session(&json, predecessor))
        .transpose()
}

fn required_session_on(
    connection: &Connection,
    session_id: SessionId,
) -> Result<Session, StoreError> {
    session_on(connection, session_id)?.ok_or(StoreError::Missing {
        kind: "session",
        id: session_id.0,
    })
}

fn direct_successor_on(
    connection: &Connection,
    session_id: SessionId,
) -> Result<Option<Session>, StoreError> {
    let row = connection
        .query_row(
            "SELECT json, continued_from FROM sessions WHERE continued_from=?1",
            [to_sql_id(session_id.0)?],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?)),
        )
        .optional()?;
    row.map(|(json, predecessor)| decode_session(&json, predecessor))
        .transpose()
}

fn ordered_lineage_on(
    connection: &Connection,
    session_id: SessionId,
) -> Result<Vec<Session>, StoreError> {
    let start = required_session_on(connection, session_id)?;
    let machine_id = start.machine_id.clone();
    let mut visited = HashSet::new();
    let mut ancestors = Vec::new();
    let mut current = start.clone();

    loop {
        remember(session_id, current.id, &mut visited)?;
        ensure_machine(&start, &current)?;
        ancestors.push(current.clone());
        let Some(predecessor) = current.continued_from else {
            break;
        };
        current =
            session_on(connection, predecessor)?.ok_or(StoreError::MissingLineageSession {
                session_id: current.id,
                missing_id: predecessor,
            })?;
    }
    ancestors.reverse();

    current = start;
    while let Some(successor) = direct_successor_on(connection, current.id)? {
        remember(session_id, successor.id, &mut visited)?;
        if successor.machine_id != machine_id {
            return Err(StoreError::CrossMachineLineage {
                source_id: current.id,
                source_machine: current.machine_id.0,
                descendant_id: successor.id,
                descendant_machine: successor.machine_id.0,
            });
        }
        if successor.continued_from != Some(current.id) {
            return Err(StoreError::ContinuationPredecessor {
                source_id: current.id,
                descendant_id: successor.id,
                actual: successor.continued_from,
            });
        }
        ancestors.push(successor.clone());
        current = successor;
    }
    Ok(ancestors)
}

fn remember(
    start: SessionId,
    candidate: SessionId,
    visited: &mut HashSet<SessionId>,
) -> Result<(), StoreError> {
    if !visited.insert(candidate) {
        return Err(StoreError::LineageCycle {
            session_id: start,
            cycle_id: candidate,
        });
    }
    if visited.len() > MAX_LINEAGE_GENERATIONS {
        return Err(StoreError::LineageTooLong {
            session_id: start,
            maximum: MAX_LINEAGE_GENERATIONS,
        });
    }
    Ok(())
}

fn ensure_machine(source: &Session, candidate: &Session) -> Result<(), StoreError> {
    if source.machine_id == candidate.machine_id {
        return Ok(());
    }
    Err(StoreError::CrossMachineLineage {
        source_id: source.id,
        source_machine: source.machine_id.0.clone(),
        descendant_id: candidate.id,
        descendant_machine: candidate.machine_id.0.clone(),
    })
}

fn sql_session_id(value: Option<i64>) -> Result<Option<SessionId>, StoreError> {
    value
        .map(|value| {
            u64::try_from(value)
                .map(SessionId)
                .map_err(|_| StoreError::InvalidStoredId { value })
        })
        .transpose()
}
