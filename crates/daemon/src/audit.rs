//! Append-only audit records for remote terminal input.

use protocol::{SessionId, Timestamp};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Metadata recorded for one remote terminal input frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditRecord {
    /// When the input arrived, in Unix milliseconds.
    pub timestamp: Timestamp,
    /// Remote socket address or other client identifier.
    pub client_addr: String,
    /// Session that received the input.
    pub session_id: SessionId,
    /// Number of input bytes, excluding the frame header.
    pub byte_count: usize,
}

/// Append-only JSON Lines audit log.
#[derive(Debug, Clone)]
pub struct AuditLog {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

/// Errors produced while appending an audit record.
#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    /// An audit filesystem operation failed.
    #[error("failed to {operation} audit log '{}': {source}", path.display())]
    File {
        /// Operation that failed.
        operation: &'static str,
        /// Audit log path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// An audit record could not be serialized.
    #[error("failed to serialize audit record for session {session_id}: {source}")]
    Serialize {
        /// Session being audited.
        session_id: SessionId,
        /// JSON serializer error.
        source: serde_json::Error,
    },
    /// The in-process audit lock was poisoned.
    #[error("failed to lock audit log '{}': another writer panicked", .0.display())]
    Lock(PathBuf),
}

impl AuditLog {
    /// Use `<data_dir>/audit.jsonl` as the append-only audit log.
    pub fn new(data_dir: &Path) -> Self {
        AuditLog {
            path: data_dir.join("audit.jsonl"),
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Append one record without storing the terminal bytes themselves.
    pub fn append(&self, record: &AuditRecord) -> Result<(), AuditError> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| AuditError::Lock(self.path.clone()))?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| AuditError::File {
                operation: "create parent directory for",
                path: self.path.clone(),
                source,
            })?;
        }
        let mut line = serde_json::to_vec(record).map_err(|source| AuditError::Serialize {
            session_id: record.session_id,
            source,
        })?;
        line.push(b'\n');
        let mut file = crate::private_fs::private_file_options()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|source| AuditError::File {
                operation: "open",
                path: self.path.clone(),
                source,
            })?;
        file.write_all(&line).map_err(|source| AuditError::File {
            operation: "append to",
            path: self.path.clone(),
            source,
        })
    }
}
