//! The worker's owner-only link file.

use crate::{link_crypto, private_fs::private_file_options};
use protocol::{MachineId, Secret, Timestamp};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, os::unix::fs::PermissionsExt, path::Path};

const VERSION: u32 = 1;

/// Persisted link state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum StoredState {
    /// Linked.
    Active,
    /// The control revoked this Machine; credentials were deleted.
    Revoked,
}

/// Contents of `link.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LinkFile {
    pub(super) version: u32,
    pub(super) state: StoredState,
    pub(super) control_url: String,
    pub(super) control_machine_id: Option<MachineId>,
    pub(super) link_id: Option<String>,
    pub(super) worker_credential: Option<Secret>,
    pub(super) control_credential_sha256: Option<String>,
    pub(super) credential_expires_at: Option<Timestamp>,
    pub(super) display_name: Option<String>,
    pub(super) paired_at: Option<Timestamp>,
    pub(super) heartbeat_interval_ms: u64,
    pub(super) sequence: u64,
    pub(super) last_heartbeat_at: Option<Timestamp>,
    #[serde(default)]
    pub(super) notification_ingress: bool,
}

impl LinkFile {
    /// A fresh active link.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn active(
        control_url: String,
        control_machine_id: MachineId,
        link_id: String,
        worker_credential: Secret,
        control_credential: &str,
        credential_expires_at: Timestamp,
        display_name: String,
        paired_at: Timestamp,
        heartbeat_interval_ms: u64,
    ) -> Self {
        LinkFile {
            version: VERSION,
            state: StoredState::Active,
            control_url,
            control_machine_id: Some(control_machine_id),
            link_id: Some(link_id),
            worker_credential: Some(worker_credential),
            control_credential_sha256: Some(link_crypto::sha256_hex(control_credential)),
            credential_expires_at: Some(credential_expires_at),
            display_name: Some(display_name),
            paired_at: Some(paired_at),
            heartbeat_interval_ms,
            sequence: 0,
            last_heartbeat_at: None,
            notification_ingress: false,
        }
    }

    /// Drop every credential after a revocation, keeping what helps the owner.
    pub(super) fn revoke(&mut self) {
        self.state = StoredState::Revoked;
        self.link_id = None;
        self.worker_credential = None;
        self.control_credential_sha256 = None;
        self.credential_expires_at = None;
        self.control_machine_id = None;
        self.paired_at = None;
        self.notification_ingress = false;
    }
}

/// Read the link file; `Ok(None)` when absent.
pub(super) fn read(path: &Path) -> Result<Option<LinkFile>, String> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read '{}': {error}", path.display())),
    };
    let mode = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions()
        .mode();
    if mode & 0o077 != 0 {
        return Err(format!(
            "'{}' must be readable only by its owner",
            path.display()
        ));
    }
    let file: LinkFile = serde_json::from_str(&text)
        .map_err(|error| format!("'{}' is invalid: {error}", path.display()))?;
    if file.version != VERSION {
        return Err(format!(
            "'{}' has unsupported version {}",
            path.display(),
            file.version
        ));
    }
    Ok(Some(file))
}

/// Atomically replace the link file with mode 0600.
pub(super) fn write(path: &Path, file: &LinkFile) -> Result<(), String> {
    let bytes = serde_json::to_vec(file).map_err(|error| error.to_string())?;
    let suffix = link_crypto::new_handle("tmp").map_err(|error| error.to_string())?;
    let temporary = path.with_extension(suffix);
    let written = private_file_options()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut handle| handle.write_all(&bytes).and_then(|()| handle.sync_all()))
        .and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = written {
        let _ = fs::remove_file(&temporary);
        return Err(format!("cannot write '{}': {error}", path.display()));
    }
    Ok(())
}

/// Remove the link file; absent is fine.
pub(super) fn remove(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove '{}': {error}", path.display())),
    }
}
