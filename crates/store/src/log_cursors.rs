//! Resumable adopted-log read positions, so a restart does not replay a log.

use crate::{to_sql_id, Store, StoreError};
use protocol::SessionId;
use rusqlite::{params, OptionalExtension};

/// Persisted read position in one adopted Session's log file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredLogCursor {
    /// Log path the cursor belongs to.
    pub path: String,
    /// Filesystem device of the file.
    pub device: u64,
    /// Inode of the file.
    pub inode: u64,
    /// Next unread byte offset.
    pub offset: u64,
}

impl Store {
    /// Insert or replace the log cursor for an existing Session.
    pub fn save_log_cursor(
        &self,
        session_id: SessionId,
        cursor: &StoredLogCursor,
    ) -> Result<(), StoreError> {
        self.lock()?.execute(
            "INSERT INTO log_cursors(session_id, path, device, inode, offset)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(session_id) DO UPDATE SET
                 path = excluded.path, device = excluded.device,
                 inode = excluded.inode, offset = excluded.offset",
            params![
                to_sql_id(session_id.0)?,
                cursor.path,
                to_bits(cursor.device),
                to_bits(cursor.inode),
                to_sql_id(cursor.offset)?,
            ],
        )?;
        Ok(())
    }

    /// Return the log cursor for `session_id`, if one was saved.
    pub fn log_cursor(&self, session_id: SessionId) -> Result<Option<StoredLogCursor>, StoreError> {
        let row = self
            .lock()?
            .query_row(
                "SELECT path, device, inode, offset FROM log_cursors WHERE session_id = ?1",
                [to_sql_id(session_id.0)?],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()?;
        row.map(|(path, device, inode, offset)| {
            Ok(StoredLogCursor {
                path,
                device: from_bits(device),
                inode: from_bits(inode),
                offset: u64::try_from(offset)
                    .map_err(|_| StoreError::InvalidStoredId { value: offset })?,
            })
        })
        .transpose()
    }

    /// Forget the log cursor for `session_id`; missing cursors are not errors.
    pub fn delete_log_cursor(&self, session_id: SessionId) -> Result<(), StoreError> {
        self.lock()?.execute(
            "DELETE FROM log_cursors WHERE session_id = ?1",
            [to_sql_id(session_id.0)?],
        )?;
        Ok(())
    }
}

/// Device and inode numbers are opaque 64-bit values; keep every bit.
fn to_bits(value: u64) -> i64 {
    i64::from_ne_bytes(value.to_ne_bytes())
}

fn from_bits(value: i64) -> u64 {
    u64::from_ne_bytes(value.to_ne_bytes())
}
