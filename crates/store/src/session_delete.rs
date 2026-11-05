//! Permanent deletion of finished Sessions and everything stored about them.

use crate::{to_sql_id, Store, StoreError};
use protocol::SessionId;
use rusqlite::{params, Transaction};

/// Tables holding one row per ID, whose IDs are never reused.
const NUMBERED: [&str; 3] = ["sessions", "commands", "events"];

impl Store {
    /// Erase Sessions and their Commands, Events, metrics, cursors and
    /// notification records in one transaction; all or nothing.
    ///
    /// Give a continuation chain newest first: a Session cannot be deleted
    /// while a later generation still points at it.
    pub fn delete_sessions(&self, sessions: &[SessionId]) -> Result<(), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        raise_id_floors(&transaction)?;
        for session in sessions {
            let id = to_sql_id(session.0)?;
            for table in [
                "notification_deliveries",
                "notification_intent_outbox",
                "events",
                "commands",
                "metrics_raw",
                "metrics_minute",
                "log_cursors",
            ] {
                transaction.execute(
                    &format!("DELETE FROM {table} WHERE session_id=?1"),
                    params![id],
                )?;
            }
            let removed = transaction.execute("DELETE FROM sessions WHERE id=?1", params![id])?;
            if removed == 0 {
                return Err(StoreError::Missing {
                    kind: "session",
                    id: session.0,
                });
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

/// Remember the highest ID of each table before its rows go away.
fn raise_id_floors(transaction: &Transaction<'_>) -> Result<(), StoreError> {
    for table in NUMBERED {
        transaction.execute(
            &format!(
                "INSERT INTO id_floors(name, value)
                 SELECT ?1, COALESCE(MAX(id), 0) FROM {table} WHERE true
                 ON CONFLICT(name) DO UPDATE SET value=MAX(value, excluded.value)"
            ),
            params![table],
        )?;
    }
    Ok(())
}
