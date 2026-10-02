//! Immutable per-Machine identity, created once and never silently replaced.
//!
//! The ID is a ULID stored owner-only beside the Machine token. Install,
//! update, rename, and reconnect never touch it. A corrupt file is reported,
//! not overwritten; only an explicit reset replaces it.

use crate::auth::secure_random;
use crate::private_fs::private_file_options;
use protocol::{link::PAIRING_CODE_ALPHABET, MachineId, Timestamp};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

const VERSION: u32 = 1;

/// This Machine's persisted identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineIdentity {
    /// Identity file format version.
    pub version: u32,
    /// Immutable Machine ID.
    pub machine_id: MachineId,
    /// When the ID was created.
    pub created_at: Timestamp,
    /// Hostname-derived ID used before Milestone 4.0, for data migration.
    pub legacy_machine_id: Option<String>,
}

/// Failure loading or creating the identity.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    /// The file exists but is not a valid identity.
    #[error("Machine identity '{}' is invalid: {reason}; run `sentinal identity reset`", path.display())]
    Corrupt {
        /// Identity file.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
    /// A filesystem operation failed.
    #[error("failed to {operation} Machine identity '{}': {source}", path.display())]
    File {
        /// Operation that failed.
        operation: &'static str,
        /// Identity file.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Secure randomness was unavailable.
    #[error("failed to generate a Machine ID: {0}")]
    Random(String),
}

/// Load the identity, creating it on first start.
pub fn load_or_create(
    path: &Path,
    now: Timestamp,
    legacy_machine_id: &str,
) -> Result<MachineIdentity, IdentityError> {
    match fs::symlink_metadata(path) {
        Ok(_) => read(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create(path, now, legacy_machine_id)
        }
        Err(source) => Err(file_error("inspect", path, source)),
    }
}

/// Move the current file aside and create a new identity.
pub fn reset(
    path: &Path,
    now: Timestamp,
    legacy_machine_id: &str,
) -> Result<MachineIdentity, IdentityError> {
    if fs::symlink_metadata(path).is_ok() {
        let aside = path.with_file_name(format!(
            "{}.corrupt-{}",
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("machine-id.json"),
            now.0
        ));
        fs::rename(path, &aside).map_err(|source| file_error("move aside", path, source))?;
    }
    create(path, now, legacy_machine_id)
}

/// Generate a ULID: 48-bit milliseconds then 80 random bits, Crockford base32.
pub fn new_ulid(now: Timestamp) -> Result<MachineId, IdentityError> {
    let random = secure_random::<10>().map_err(|error| IdentityError::Random(error.to_string()))?;
    let millis = u128::from(u64::try_from(now.0).unwrap_or(0) & 0xFFFF_FFFF_FFFF);
    let mut value = millis << 80;
    for (index, byte) in random.iter().enumerate() {
        value |= u128::from(*byte) << (8 * (9 - index));
    }
    let mut text = [0_u8; 26];
    for (index, slot) in text.iter_mut().enumerate() {
        let shift = 5 * (25 - index);
        *slot = PAIRING_CODE_ALPHABET[((value >> shift) & 0x1F) as usize];
    }
    Ok(MachineId(String::from_utf8_lossy(&text).into_owned()))
}

fn read(path: &Path) -> Result<MachineIdentity, IdentityError> {
    let corrupt = |reason: String| IdentityError::Corrupt {
        path: path.to_path_buf(),
        reason,
    };
    let metadata =
        fs::symlink_metadata(path).map_err(|source| file_error("inspect", path, source))?;
    if !metadata.is_file() {
        return Err(corrupt("is not a regular file".into()));
    }
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(corrupt(format!(
            "mode is {:o}; it must be readable only by its owner",
            metadata.permissions().mode() & 0o777
        )));
    }
    let text = fs::read_to_string(path).map_err(|source| file_error("read", path, source))?;
    let identity: MachineIdentity =
        serde_json::from_str(&text).map_err(|error| corrupt(error.to_string()))?;
    if identity.version != VERSION {
        return Err(corrupt(format!("unsupported version {}", identity.version)));
    }
    if !identity.machine_id.is_ulid() {
        return Err(corrupt("machine_id is not a ULID".into()));
    }
    Ok(identity)
}

fn create(
    path: &Path,
    now: Timestamp,
    legacy_machine_id: &str,
) -> Result<MachineIdentity, IdentityError> {
    let identity = MachineIdentity {
        version: VERSION,
        machine_id: new_ulid(now)?,
        created_at: now,
        legacy_machine_id: Some(legacy_machine_id.to_string()),
    };
    let bytes =
        serde_json::to_vec(&identity).map_err(|error| IdentityError::Random(error.to_string()))?;
    if let Some(parent) = path.parent() {
        crate::private_fs::ensure_private_dir(parent)
            .map_err(|source| file_error("create directory for", path, source))?;
    }
    // create_new: never replace an identity another process just wrote.
    let mut file = private_file_options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| file_error("create", path, source))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|source| file_error("write", path, source))?;
    Ok(identity)
}

fn file_error(operation: &'static str, path: &Path, source: std::io::Error) -> IdentityError {
    IdentityError::File {
        operation,
        path: path.to_path_buf(),
        source,
    }
}
