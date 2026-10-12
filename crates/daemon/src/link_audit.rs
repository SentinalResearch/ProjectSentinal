//! Metadata-only audit records for pairing, registry, and link events.
//!
//! Records never contain pairing codes, credentials, hashes, Authorization
//! headers, or request bodies.

use crate::private_fs::ensure_private_dir;
use protocol::{MachineId, Timestamp};
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// One audit line.
#[derive(Debug, Default, Serialize)]
pub struct LinkAuditRecord<'a> {
    /// When it happened.
    pub at: i64,
    /// What happened, e.g. `EXCHANGE_SUCCEEDED`.
    pub kind: &'a str,
    /// Pairing-code handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_id: Option<&'a str>,
    /// Approval-request handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<&'a str>,
    /// Machine concerned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine_id: Option<&'a str>,
    /// Link handle.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_id: Option<&'a str>,
    /// Tailscale user or address of the requester.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peer: Option<&'a str>,
    /// Failure or change detail (never secret).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'a str>,
}

impl LinkAuditRecord<'_> {
    /// Start a record of `kind` at `at`.
    pub fn new(at: Timestamp, kind: &str) -> LinkAuditRecord<'_> {
        LinkAuditRecord {
            at: at.0,
            kind,
            ..LinkAuditRecord::default()
        }
    }
}

/// Append-only owner-only JSON Lines audit.
#[derive(Debug, Clone)]
pub struct LinkAudit {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl LinkAudit {
    /// Write to `<dir>/<file_name>`.
    pub fn new(dir: &Path, file_name: &str) -> Self {
        LinkAudit {
            path: dir.join(file_name),
            lock: Arc::new(Mutex::new(())),
        }
    }

    /// Append one record; failures are logged, never fatal.
    pub fn append(&self, record: &LinkAuditRecord<'_>) {
        if let Err(error) = self.try_append(record) {
            tracing::warn!(path = %self.path.display(), %error, "failed to append link audit");
        }
    }

    /// Convenience for a record about one Machine.
    pub fn machine(&self, at: Timestamp, kind: &str, machine_id: &MachineId, reason: Option<&str>) {
        self.append(&LinkAuditRecord {
            machine_id: Some(&machine_id.0),
            reason,
            ..LinkAuditRecord::new(at, kind)
        });
    }

    fn try_append(&self, record: &LinkAuditRecord<'_>) -> Result<(), String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "audit lock poisoned".to_string())?;
        if let Some(parent) = self.path.parent() {
            ensure_private_dir(parent).map_err(|error| error.to_string())?;
        }
        let mut line = serde_json::to_vec(record).map_err(|error| error.to_string())?;
        line.push(b'\n');
        crate::private_fs::append_bounded(
            &self.path,
            &line,
            crate::private_fs::AUDIT_ROTATE_BYTES,
            crate::private_fs::AUDIT_KEEP_ROTATED,
        )
        .map_err(|error| error.to_string())
    }
}
