//! SQLite persistence for Machines, Sessions, Commands, Events, and Metrics.

mod metrics;
mod records;

use protocol::{Event, EventId, EventKind, Machine, Session, SessionId, SessionState, Timestamp};
use rusqlite::{Connection, OptionalExtension};
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
};

pub use metrics::{MinuteMetric, RawMetricSample};

const MIGRATIONS: &[(u32, &str)] = &[(1, include_str!("../migrations/001_initial.sql"))];

/// Errors reading or writing structured state.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite rejected an operation.
    #[error("SQLite store operation failed: {0}")]
    Sql(#[from] rusqlite::Error),
    /// Persisted JSON could not be encoded or decoded.
    #[error("store JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),
    /// A persisted Session could not make the required lifecycle transition.
    #[error(transparent)]
    Protocol(#[from] protocol::ProtocolError),
    /// Shared connection state was poisoned by a panic.
    #[error("failed to lock SQLite store: lock poisoned")]
    Lock,
    /// An unsigned wire identifier cannot be represented by SQLite.
    #[error("wire identifier {value} is outside SQLite's signed integer range")]
    IdRange {
        /// Identifier that could not be stored.
        value: u64,
    },
    /// An update targeted a record that does not exist.
    #[error("store has no {kind} {id}")]
    Missing {
        /// Record category.
        kind: &'static str,
        /// Missing record identifier.
        id: u64,
    },
}

/// Thread-safe connection to one terminal-agent SQLite database.
pub struct Store {
    pub(crate) connection: Mutex<Connection>,
}

impl Store {
    /// Open or create a database and run pending numbered migrations.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    /// Create a migrated in-memory database for tests and ephemeral use.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, StoreError> {
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version (version INTEGER PRIMARY KEY NOT NULL);",
        )?;
        let current: u32 = connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )?;
        for (version, sql) in MIGRATIONS.iter().filter(|(version, _)| *version > current) {
            let transaction = connection.transaction()?;
            transaction.execute_batch(sql)?;
            transaction.execute("INSERT INTO schema_version(version) VALUES (?1)", [version])?;
            transaction.commit()?;
        }
        Ok(Store {
            connection: Mutex::new(connection),
        })
    }

    /// Return the latest applied migration number.
    pub fn schema_version(&self) -> Result<u32, StoreError> {
        Ok(self.lock()?.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )?)
    }

    /// Return SQLite's active journal mode.
    pub fn journal_mode(&self) -> Result<String, StoreError> {
        Ok(self
            .lock()?
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))?)
    }

    /// Check whether a table exists; intended for migration diagnostics.
    pub fn has_table(&self, name: &str) -> Result<bool, StoreError> {
        Ok(self
            .lock()?
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?1",
                [name],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// Insert or replace a Machine snapshot.
    pub fn upsert_machine(&self, machine: &Machine) -> Result<(), StoreError> {
        self.lock()?.execute(
            "INSERT INTO machines(id, json) VALUES (?1, ?2) ON CONFLICT(id) DO UPDATE SET json=excluded.json",
            (&machine.id.0, serde_json::to_string(machine)?),
        )?;
        Ok(())
    }

    /// Insert a Session snapshot.
    pub fn insert_session(&self, session: &Session) -> Result<(), StoreError> {
        session.validate()?;
        self.lock()?.execute(
            "INSERT INTO sessions(id, state, updated_at, json) VALUES (?1, ?2, ?3, ?4)",
            (
                to_sql_id(session.id.0)?,
                state_name(session.state)?,
                session.updated_at.0,
                serde_json::to_string(session)?,
            ),
        )?;
        Ok(())
    }

    /// Update an existing Session snapshot.
    pub fn update_session(&self, session: &Session) -> Result<(), StoreError> {
        session.validate()?;
        let changed = self.lock()?.execute(
            "UPDATE sessions SET state=?1, updated_at=?2, json=?3 WHERE id=?4",
            (
                state_name(session.state)?,
                session.updated_at.0,
                serde_json::to_string(session)?,
                to_sql_id(session.id.0)?,
            ),
        )?;
        if changed == 0 {
            return Err(StoreError::Missing {
                kind: "session",
                id: session.id.0,
            });
        }
        Ok(())
    }

    /// Load one Session snapshot.
    pub fn session(&self, id: SessionId) -> Result<Option<Session>, StoreError> {
        let json: Option<String> = self
            .lock()?
            .query_row(
                "SELECT json FROM sessions WHERE id=?1",
                [to_sql_id(id.0)?],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| decode_session(&value)).transpose()
    }

    /// Mark all persisted live Sessions interrupted after a daemon restart.
    pub fn interrupt_orphaned_sessions(&self, at: Timestamp) -> Result<Vec<Event>, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let json_values = {
            let mut statement = transaction.prepare(
                "SELECT json FROM sessions WHERE state NOT IN ('COMPLETED','FAILED','INTERRUPTED') ORDER BY id",
            )?;
            let values = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            values
        };
        let mut next_event: u64 =
            transaction.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM events", [], |row| {
                row.get(0)
            })?;
        let mut events = Vec::with_capacity(json_values.len());
        for json in json_values {
            let mut session = decode_session(&json)?;
            let command_id = session.current_command;
            session.transition(SessionState::Interrupted, at)?;
            let event = Event {
                id: EventId(next_event),
                session_id: session.id,
                command_id,
                at,
                kind: EventKind::WarningDetected {
                    message: "session interrupted because its PTY did not survive daemon restart"
                        .into(),
                },
            };
            next_event = next_event.saturating_add(1);
            transaction.execute(
                "UPDATE sessions SET state=?1, updated_at=?2, json=?3 WHERE id=?4",
                (
                    state_name(session.state)?,
                    at.0,
                    serde_json::to_string(&session)?,
                    to_sql_id(session.id.0)?,
                ),
            )?;
            records::insert_event_on(&transaction, &event)?;
            events.push(event);
        }
        transaction.commit()?;
        Ok(events)
    }

    /// Return the first unused Session ID.
    pub fn next_session_id(&self) -> Result<u64, StoreError> {
        self.next_id("sessions")
    }

    /// Return the first unused Command ID.
    pub fn next_command_id(&self) -> Result<u64, StoreError> {
        self.next_id("commands")
    }

    /// Return the first unused Event ID.
    pub fn next_event_id(&self) -> Result<u64, StoreError> {
        self.next_id("events")
    }

    fn next_id(&self, table: &'static str) -> Result<u64, StoreError> {
        let sql = match table {
            "sessions" => "SELECT COALESCE(MAX(id), 0) + 1 FROM sessions",
            "commands" => "SELECT COALESCE(MAX(id), 0) + 1 FROM commands",
            "events" => "SELECT COALESCE(MAX(id), 0) + 1 FROM events",
            _ => {
                return Err(StoreError::Missing {
                    kind: "table",
                    id: 0,
                })
            }
        };
        Ok(self.lock()?.query_row(sql, [], |row| row.get(0))?)
    }

    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::Lock)
    }
}

fn decode_session(json: &str) -> Result<Session, StoreError> {
    let session: Session = serde_json::from_str(json)?;
    session.validate()?;
    Ok(session)
}

fn state_name(state: SessionState) -> Result<String, StoreError> {
    let json = serde_json::to_string(&state)?;
    Ok(json.trim_matches('"').to_string())
}

pub(crate) fn to_sql_id(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::IdRange { value })
}
