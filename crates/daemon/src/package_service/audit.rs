//! Metadata-only audit records for package downloads.

use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Serialize)]
struct PackageAuditRecord<'a> {
    at: i64,
    route: &'a str,
    release: Option<&'a str>,
    target: Option<&'a str>,
    status: u16,
    bytes_sent: u64,
}

/// Append-only package-request audit without headers, queries, or payloads.
#[derive(Debug, Clone)]
pub(super) struct PackageAudit {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl PackageAudit {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            path: root.join("package-audit.jsonl"),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub(super) fn append(
        &self,
        route: &str,
        release: Option<&str>,
        target: Option<&str>,
        status: u16,
        bytes_sent: u64,
    ) {
        if let Err(error) = self.try_append(route, release, target, status, bytes_sent) {
            tracing::warn!(%error, "failed to append package audit metadata");
        }
    }

    fn try_append(
        &self,
        route: &str,
        release: Option<&str>,
        target: Option<&str>,
        status: u16,
        bytes_sent: u64,
    ) -> Result<(), String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "package audit lock was poisoned".to_string())?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        let record = PackageAuditRecord {
            at: i64::try_from(at).unwrap_or(i64::MAX),
            route,
            release,
            target,
            status,
            bytes_sent,
        };
        let mut bytes = serde_json::to_vec(&record).map_err(|error| error.to_string())?;
        bytes.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .map_err(|error| error.to_string())?;
        file.write_all(&bytes).map_err(|error| error.to_string())
    }
}
