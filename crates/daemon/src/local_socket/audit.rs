//! Metadata-only security audit for rejected local peers.

use super::LocalSocketError;
use protocol::Timestamp;
use serde::Serialize;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

/// Metadata-only log for rejected local peers and unsafe ingress activity.
#[derive(Debug, Clone)]
pub struct SecurityAudit {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

#[derive(Serialize)]
struct SecurityAuditRecord<'a> {
    timestamp: Timestamp,
    kind: &'a str,
    peer_uid: Option<u32>,
    socket_path: &'a Path,
    reason: &'a str,
}

impl SecurityAudit {
    /// Write security metadata to `<data_dir>/security-audit.jsonl`.
    pub fn new(data_dir: &Path) -> Self {
        SecurityAudit {
            path: data_dir.join("security-audit.jsonl"),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub(super) fn append(
        &self,
        kind: &'static str,
        peer_uid: Option<u32>,
        socket_path: &Path,
        reason: &str,
    ) -> Result<(), LocalSocketError> {
        let _guard = self.lock.lock().map_err(|_| LocalSocketError::AuditLock)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|source| LocalSocketError::Io {
                operation: "create security audit directory for",
                path: self.path.clone(),
                source,
            })?;
        }
        let mut line = serde_json::to_vec(&SecurityAuditRecord {
            timestamp: system_timestamp(),
            kind,
            peer_uid,
            socket_path,
            reason,
        })?;
        line.push(b'\n');
        let mut file = crate::private_fs::private_file_options()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|source| LocalSocketError::Io {
                operation: "open security audit log for",
                path: self.path.clone(),
                source,
            })?;
        file.write_all(&line)
            .map_err(|source| LocalSocketError::Io {
                operation: "append security audit log for",
                path: self.path.clone(),
                source,
            })
    }
}

fn system_timestamp() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    Timestamp(i64::try_from(millis).unwrap_or(i64::MAX))
}
