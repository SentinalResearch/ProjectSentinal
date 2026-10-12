//! SQLite persistence for Machines, Sessions, Commands, Events, and Metrics.

mod error;
mod lineage;
mod log_cursors;
mod metrics;
mod native_push;
mod notification_deliveries;
mod projection;
mod records;
pub mod registry;

use protocol::{
    CaptureLevel, Event, EventId, EventKind, Machine, Session, SessionId, SessionState, Timestamp,
};
use rusqlite::{Connection, OptionalExtension};
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    sync::{Mutex, MutexGuard},
};

pub use error::StoreError;
pub use log_cursors::StoredLogCursor;
pub use metrics::{MinuteMetric, RawMetricSample};
pub use native_push::{
    NewNotificationIntentOutbox, NotificationIntentOutboxInsert, PushDelivery,
    StoredNotificationIntent, StoredPushSubscription,
};
pub use notification_deliveries::{NewNotificationDelivery, NotificationDelivery};
pub use projection::{ControlProjectionState, ProjectionOutboxLimits};

const MIGRATIONS: &[(u32, &str)] = &[
    (1, include_str!("../migrations/001_initial.sql")),
    (2, include_str!("../migrations/002_session_lineage.sql")),
    (3, include_str!("../migrations/003_log_cursors.sql")),
    (4, include_str!("../migrations/004_machine_registry.sql")),
    (
        5,
        include_str!("../migrations/005_notification_deliveries.sql"),
    ),
    (6, include_str!("../migrations/006_native_push.sql")),
    (7, include_str!("../migrations/007_projection.sql")),
];

/// Thread-safe connection to one terminal-agent SQLite database.
pub struct Store {
    pub(crate) connection: Mutex<Connection>,
}

/// Create the database file as `0600` and tighten an existing one, before
/// SQLite opens it. SQLite gives its `-wal` and `-shm` files the same mode.
fn restrict_to_owner(path: &Path) -> Result<(), StoreError> {
    let file_error = |source| StoreError::File {
        path: path.to_path_buf(),
        source,
    };
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)
        .map_err(file_error)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).map_err(file_error)
}

impl Store {
    /// Open or create an owner-only database and run pending numbered migrations.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        restrict_to_owner(path)?;
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
        if session.continued_from.is_some() {
            return Err(StoreError::LineageInsertRequired {
                session_id: session.id,
            });
        }
        self.lock()?.execute(
            "INSERT INTO sessions(id, state, updated_at, json, continued_from) VALUES (?1, ?2, ?3, ?4, NULL)",
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
        let connection = self.lock()?;
        let stored_lineage = lineage::continued_from_on(&connection, session.id)?;
        if stored_lineage != session.continued_from {
            return Err(StoreError::LineageMismatch {
                session_id: session.id,
                column: stored_lineage,
                json: session.continued_from,
            });
        }
        let changed = connection.execute(
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
        let connection = self.lock()?;
        lineage::session_on(&connection, id)
    }

    /// Load every persisted Session snapshot ordered by ID.
    pub fn sessions(&self) -> Result<Vec<Session>, StoreError> {
        let connection = self.lock()?;
        let mut statement =
            connection.prepare("SELECT json, continued_from FROM sessions ORDER BY id")?;
        let values = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        values
            .iter()
            .map(|(json, predecessor)| decode_session(json, *predecessor))
            .collect()
    }

    /// Mark all persisted live Sessions interrupted after a daemon restart.
    pub fn interrupt_orphaned_sessions(&self, at: Timestamp) -> Result<Vec<Event>, StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let json_values = {
            let mut statement = transaction.prepare(
                "SELECT json, continued_from FROM sessions WHERE state NOT IN ('COMPLETED','FAILED','INTERRUPTED') ORDER BY id",
            )?;
            let values = statement
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            values
        };
        let mut next_event: u64 =
            transaction.query_row("SELECT COALESCE(MAX(id), 0) + 1 FROM events", [], |row| {
                row.get(0)
            })?;
        let mut events = Vec::with_capacity(json_values.len());
        for (json, predecessor) in json_values {
            let mut session = decode_session(&json, predecessor)?;
            // Only daemon-owned PTYs die with the daemon; reduced-capture
            // Sessions are verified by the daemon's external recovery instead.
            if session.capture != CaptureLevel::Full {
                continue;
            }
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

    /// Return every Session that has not reached an end state, ordered by ID.
    pub fn live_sessions(&self) -> Result<Vec<Session>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT json, continued_from FROM sessions WHERE state NOT IN ('COMPLETED','FAILED','INTERRUPTED') ORDER BY id",
        )?;
        let values = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        values
            .iter()
            .map(|(json, predecessor)| decode_session(json, *predecessor))
            .collect()
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

pub(crate) fn decode_session(
    json: &str,
    continued_from: Option<i64>,
) -> Result<Session, StoreError> {
    let session: Session = serde_json::from_str(json)?;
    session.validate()?;
    let column = continued_from
        .map(|value| {
            u64::try_from(value)
                .map(SessionId)
                .map_err(|_| StoreError::InvalidStoredId { value })
        })
        .transpose()?;
    if session.continued_from != column {
        return Err(StoreError::LineageMismatch {
            session_id: session.id,
            column,
            json: session.continued_from,
        });
    }
    Ok(session)
}

fn state_name(state: SessionState) -> Result<String, StoreError> {
    let json = serde_json::to_string(&state)?;
    Ok(json.trim_matches('"').to_string())
}

pub(crate) fn to_sql_id(value: u64) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::IdRange { value })
}
