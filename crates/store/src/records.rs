//! Command and Event persistence queries.

use crate::{to_sql_id, Store, StoreError};
use protocol::{
    Command, CommandClass, CommandId, Event, EventId, EventKind, MachineId, SessionId, Timestamp,
};
use rusqlite::{params, Connection, Row};

impl Store {
    /// Insert a newly started Command.
    pub fn insert_command(&self, command: &Command) -> Result<(), StoreError> {
        self.lock()?.execute(
            "INSERT INTO commands(id, session_id, machine_id, text, cwd, started_at, finished_at, exit_code, class)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                to_sql_id(command.id.0)?,
                to_sql_id(command.session_id.0)?,
                command.machine_id.0,
                command.text,
                command.cwd,
                command.started_at.0,
                command.finished_at.map(|value| value.0),
                command.exit_code,
                class_json(command.class)?,
            ],
        )?;
        Ok(())
    }

    /// Persist a Command's completion fields and final text.
    pub fn finish_command(&self, command: &Command) -> Result<(), StoreError> {
        let changed = self.lock()?.execute(
            "UPDATE commands SET text=?1, cwd=?2, finished_at=?3, exit_code=?4, class=?5 WHERE id=?6",
            params![
                command.text,
                command.cwd,
                command.finished_at.map(|value| value.0),
                command.exit_code,
                class_json(command.class)?,
                to_sql_id(command.id.0)?,
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::Missing {
                kind: "command",
                id: command.id.0,
            });
        }
        Ok(())
    }

    /// Return recent Commands, optionally restricted to one Session.
    pub fn history(
        &self,
        session: Option<SessionId>,
        limit: u32,
    ) -> Result<Vec<Command>, StoreError> {
        let connection = self.lock()?;
        let (sql, session_id) = match session {
            Some(id) => (
                "SELECT id, session_id, machine_id, text, cwd, started_at, finished_at, exit_code, class
                 FROM commands WHERE session_id=?1 ORDER BY started_at DESC, id DESC LIMIT ?2",
                Some(to_sql_id(id.0)?),
            ),
            None => (
                "SELECT id, session_id, machine_id, text, cwd, started_at, finished_at, exit_code, class
                 FROM commands ORDER BY started_at DESC, id DESC LIMIT ?2",
                None,
            ),
        };
        let mut statement = connection.prepare(sql)?;
        let rows = statement.query_map(params![session_id, limit], command_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Search Command text through SQLite FTS5.
    pub fn search_commands(&self, query: &str, limit: u32) -> Result<Vec<Command>, StoreError> {
        if query.trim().is_empty() {
            return self.history(None, limit);
        }
        let quoted = format!("\"{}\"", query.trim().replace('"', "\"\""));
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT c.id, c.session_id, c.machine_id, c.text, c.cwd, c.started_at, c.finished_at, c.exit_code, c.class
             FROM commands_fts f JOIN commands c ON c.id=f.rowid
             WHERE commands_fts MATCH ?1 ORDER BY c.started_at DESC, c.id DESC LIMIT ?2",
        )?;
        let rows = statement.query_map(params![quoted, limit], command_from_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Insert one structured Event.
    pub fn insert_event(&self, event: &Event) -> Result<(), StoreError> {
        let connection = self.lock()?;
        insert_event_on(&connection, event)
    }

    /// Return Events with IDs greater than `after`, ordered oldest first.
    pub fn events_since(&self, after: EventId) -> Result<Vec<Event>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, session_id, command_id, at, kind_json FROM events WHERE id>?1 ORDER BY id",
        )?;
        let rows = statement.query_map([to_sql_id(after.0)?], |row| {
            let kind_json: String = row.get(4)?;
            let kind: EventKind = serde_json::from_str(&kind_json).map_err(json_to_sql)?;
            Ok(Event {
                id: EventId(from_sql_id(row.get(0)?)?),
                session_id: SessionId(from_sql_id(row.get(1)?)?),
                command_id: row
                    .get::<_, Option<i64>>(2)?
                    .map(from_sql_id)
                    .transpose()?
                    .map(CommandId),
                at: Timestamp(row.get(3)?),
                kind,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

pub(crate) fn insert_event_on(connection: &Connection, event: &Event) -> Result<(), StoreError> {
    connection.execute(
        "INSERT INTO events(id, session_id, command_id, at, kind_json) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            to_sql_id(event.id.0)?,
            to_sql_id(event.session_id.0)?,
            event.command_id.map(|id| to_sql_id(id.0)).transpose()?,
            event.at.0,
            serde_json::to_string(&event.kind)?,
        ],
    )?;
    Ok(())
}

fn command_from_row(row: &Row<'_>) -> rusqlite::Result<Command> {
    let class = row
        .get::<_, Option<String>>(8)?
        .map(|json| serde_json::from_str::<CommandClass>(&json).map_err(json_to_sql))
        .transpose()?;
    Ok(Command {
        id: CommandId(from_sql_id(row.get(0)?)?),
        session_id: SessionId(from_sql_id(row.get(1)?)?),
        machine_id: MachineId(row.get(2)?),
        text: row.get(3)?,
        cwd: row.get(4)?,
        started_at: Timestamp(row.get(5)?),
        finished_at: row.get::<_, Option<i64>>(6)?.map(Timestamp),
        exit_code: row.get(7)?,
        class,
    })
}

fn class_json(class: Option<CommandClass>) -> Result<Option<String>, StoreError> {
    class
        .map(|value| serde_json::to_string(&value).map_err(StoreError::from))
        .transpose()
}

fn from_sql_id(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(error),
        )
    })
}

fn json_to_sql(error: serde_json::Error) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
}
