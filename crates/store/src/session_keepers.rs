//! The keeper behind each keeper-held Session (T5, migration 016).

use crate::{Store, StoreError};
use protocol::{Event, EventId, EventKind, SessionId, SessionState, Timestamp};
use rusqlite::{params, OptionalExtension};

/// One row of `session_keepers`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredKeeper {
    /// The Session.
    pub session_id: u64,
    /// Its socket.
    pub socket_path: String,
    /// The per-keeper secret, hex.
    pub secret_hex: String,
    /// The shell hook nonce.
    pub osc_nonce: String,
    /// The keeper's PID.
    pub keeper_pid: u32,
    /// Its start time, seconds.
    pub keeper_started_at: u64,
    /// The shell's PID.
    pub shell_pid: u32,
    /// Its start time, seconds.
    pub shell_started_at: u64,
    /// Its protocol.
    pub protocol: u32,
    /// Its systemd scope, if any.
    pub scope_unit: Option<String>,
    /// When it was recorded, Unix milliseconds.
    pub created_at: i64,
}

fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredKeeper> {
    Ok(StoredKeeper {
        session_id: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
        socket_path: row.get(1)?,
        secret_hex: row.get(2)?,
        osc_nonce: row.get(3)?,
        keeper_pid: u32::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
        keeper_started_at: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
        shell_pid: u32::try_from(row.get::<_, i64>(6)?).unwrap_or(0),
        shell_started_at: u64::try_from(row.get::<_, i64>(7)?).unwrap_or(0),
        protocol: u32::try_from(row.get::<_, i64>(8)?).unwrap_or(0),
        scope_unit: row.get(9)?,
        created_at: row.get(10)?,
    })
}

const COLUMNS: &str = "session_id, socket_path, secret_hex, osc_nonce, keeper_pid,
    keeper_started_at, shell_pid, shell_started_at, protocol, scope_unit, created_at";

impl Store {
    /// Record (or replace) the keeper of a Session.
    pub fn put_keeper(&self, keeper: &StoredKeeper) -> Result<(), StoreError> {
        self.lock()?.execute(
            &format!("INSERT OR REPLACE INTO session_keepers ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"),
            params![
                i64::try_from(keeper.session_id).unwrap_or(i64::MAX),
                keeper.socket_path,
                keeper.secret_hex,
                keeper.osc_nonce,
                i64::from(keeper.keeper_pid),
                i64::try_from(keeper.keeper_started_at).unwrap_or(0),
                i64::from(keeper.shell_pid),
                i64::try_from(keeper.shell_started_at).unwrap_or(0),
                i64::from(keeper.protocol),
                keeper.scope_unit,
                keeper.created_at,
            ],
        )?;
        Ok(())
    }

    /// The keeper of a Session, if it has one.
    pub fn keeper(&self, session_id: SessionId) -> Result<Option<StoredKeeper>, StoreError> {
        Ok(self
            .lock()?
            .query_row(
                &format!("SELECT {COLUMNS} FROM session_keepers WHERE session_id = ?1"),
                params![i64::try_from(session_id.0).unwrap_or(i64::MAX)],
                read,
            )
            .optional()?)
    }

    /// Keepers of Sessions that have not ended, by Session ID.
    pub fn live_keepers(&self) -> Result<Vec<StoredKeeper>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM session_keepers WHERE session_id IN
               (SELECT id FROM sessions WHERE state NOT IN ('COMPLETED','FAILED','INTERRUPTED'))
             ORDER BY session_id"
        ))?;
        let rows = statement
            .query_map([], read)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Forget a Session's keeper.
    pub fn delete_keeper(&self, session_id: SessionId) -> Result<(), StoreError> {
        self.lock()?.execute(
            "DELETE FROM session_keepers WHERE session_id = ?1",
            params![i64::try_from(session_id.0).unwrap_or(i64::MAX)],
        )?;
        Ok(())
    }

    /// Mark one live Session interrupted with `reason` (a keeper that could
    /// not be reclaimed), forget its keeper, and return the Event.
    pub fn interrupt_session(
        &self,
        session_id: SessionId,
        at: Timestamp,
        reason: &str,
    ) -> Result<Option<Event>, StoreError> {
        let Some(mut session) = self.session(session_id)? else {
            return Ok(None);
        };
        self.delete_keeper(session_id)?;
        if session.state.is_finished() {
            return Ok(None);
        }
        let command_id = session.current_command;
        session.transition(SessionState::Interrupted, at)?;
        self.update_session(&session)?;
        let event = Event {
            id: EventId(self.next_event_id()?),
            session_id,
            command_id,
            at,
            kind: EventKind::WarningDetected {
                message: reason.to_string(),
            },
        };
        self.insert_event(&event)?;
        Ok(Some(event))
    }
}
