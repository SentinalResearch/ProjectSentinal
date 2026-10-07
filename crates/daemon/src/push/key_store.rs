//! Owner-only VAPID signing-key lifecycle for the control Machine.

use base64ct::{Base64UrlUnpadded, Encoding as _};
use protocol::{PushPublicKey, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use web_push_native::{
    jwt_simple::algorithms::ES256KeyPair,
    p256::{elliptic_curve::sec1::ToEncodedPoint, SecretKey},
};

const VERSION: u32 = 1;
const MAX_KEY_FILE_BYTES: u64 = 4 * 1024;

#[derive(Clone, Serialize, Deserialize)]
struct StoredVapidKey {
    version: u32,
    key_id: String,
    private_key: String,
    public_key: String,
    subject: String,
    created_at: Timestamp,
}

/// Validated VAPID key pair loaded only by the control role.
#[derive(Clone)]
pub struct VapidKeyStore {
    stored: StoredVapidKey,
}

impl std::fmt::Debug for VapidKeyStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VapidKeyStore")
            .field("version", &self.stored.version)
            .field("key_id", &self.stored.key_id)
            .field("private_key", &"[redacted]")
            .field("public_key", &self.stored.public_key)
            .field("subject", &self.stored.subject)
            .field("created_at", &self.stored.created_at)
            .finish()
    }
}

/// Failure loading, validating, or atomically creating VAPID material.
#[derive(Debug, thiserror::Error)]
pub enum VapidKeyError {
    /// Filesystem security or I/O failure.
    #[error("failed to {operation} VAPID key '{}': {reason}", path.display())]
    File {
        /// Operation being attempted.
        operation: &'static str,
        /// Key file path.
        path: PathBuf,
        /// Secret-free reason.
        reason: String,
    },
    /// Stored key data failed validation.
    #[error("VAPID key '{}' is invalid: {reason}", path.display())]
    Invalid {
        /// Key file path.
        path: PathBuf,
        /// Secret-free reason.
        reason: &'static str,
    },
    /// OS randomness did not produce a valid P-256 scalar within a bounded loop.
    #[error("failed to generate a valid VAPID P-256 key")]
    Generation,
}

impl VapidKeyStore {
    /// Load the stable key or create it once with owner-only permissions.
    pub fn load_or_create(
        path: &Path,
        subject: &str,
        owner_uid: u32,
        now: Timestamp,
    ) -> Result<Self, VapidKeyError> {
        match fs::symlink_metadata(path) {
            Ok(_) => read_key(path, subject, owner_uid),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                create_key(path, subject, owner_uid, now)
            }
            Err(error) => Err(file_error("inspect", path, error.to_string())),
        }
    }

    /// Return only public key material for an authenticated owner browser.
    pub fn public(&self) -> PushPublicKey {
        PushPublicKey {
            key_id: self.stored.key_id.clone(),
            public_key: self.stored.public_key.clone(),
            subject_origin: self.stored.subject.clone(),
        }
    }

    /// Reconstruct the signing key for one provider request.
    pub(crate) fn signing_key(&self) -> Result<ES256KeyPair, VapidKeyError> {
        let bytes = Base64UrlUnpadded::decode_vec(&self.stored.private_key)
            .map_err(|_| invalid(Path::new("[loaded]"), "private key base64 is invalid"))?;
        ES256KeyPair::from_bytes(&bytes)
            .map_err(|_| invalid(Path::new("[loaded]"), "private key scalar is invalid"))
    }

    /// VAPID subject bound to the validated control origin.
    pub(crate) fn subject(&self) -> &str {
        &self.stored.subject
    }
}

fn read_key(path: &Path, subject: &str, owner_uid: u32) -> Result<VapidKeyStore, VapidKeyError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| file_error("inspect", path, error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid(path, "expected a regular non-symlink file"));
    }
    if metadata.uid() != owner_uid {
        return Err(invalid(path, "file is owned by another user"));
    }
    if metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(invalid(path, "file mode must be 0600"));
    }
    if metadata.len() > MAX_KEY_FILE_BYTES {
        return Err(invalid(path, "file exceeds 4096 bytes"));
    }
    let bytes = fs::read(path).map_err(|error| file_error("read", path, error.to_string()))?;
    let stored: StoredVapidKey =
        serde_json::from_slice(&bytes).map_err(|_| invalid(path, "JSON is malformed"))?;
    validate(path, subject, &stored)?;
    Ok(VapidKeyStore { stored })
}

fn create_key(
    path: &Path,
    subject: &str,
    owner_uid: u32,
    now: Timestamp,
) -> Result<VapidKeyStore, VapidKeyError> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid(path, "path has no parent directory"))?;
    crate::private_fs::ensure_private_dir(parent)
        .map_err(|error| file_error("secure parent directory for", path, error.to_string()))?;
    let parent_metadata = fs::symlink_metadata(parent)
        .map_err(|error| file_error("inspect parent of", path, error.to_string()))?;
    if !parent_metadata.is_dir()
        || parent_metadata.file_type().is_symlink()
        || parent_metadata.uid() != owner_uid
        || parent_metadata.permissions().mode() & 0o777 != 0o700
    {
        return Err(invalid(
            path,
            "parent must be an owner-controlled mode-0700 directory",
        ));
    }
    let stored = generate(subject, now)?;
    let bytes = serde_json::to_vec(&stored)
        .map_err(|error| file_error("encode", path, error.to_string()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("push-vapid.json");
    let temporary = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    let mut options = crate::private_fs::private_file_options();
    let mut file = options
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| file_error("create temporary", path, error.to_string()))?;
    if let Err(error) = file.write_all(&bytes).and_then(|()| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(file_error("write temporary", path, error.to_string()));
    }
    match fs::hard_link(&temporary, path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&temporary);
            return read_key(path, subject, owner_uid);
        }
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(file_error("publish", path, error.to_string()));
        }
    }
    fs::remove_file(&temporary)
        .map_err(|error| file_error("remove temporary", path, error.to_string()))?;
    if let Ok(directory) = fs::File::open(parent) {
        directory
            .sync_all()
            .map_err(|error| file_error("sync parent of", path, error.to_string()))?;
    }
    read_key(path, subject, owner_uid)
}

fn generate(subject: &str, now: Timestamp) -> Result<StoredVapidKey, VapidKeyError> {
    for _ in 0..16 {
        let bytes = crate::auth::secure_random::<32>().map_err(|_| VapidKeyError::Generation)?;
        let Ok(secret) = SecretKey::from_slice(&bytes) else {
            continue;
        };
        let public = secret.public_key().to_encoded_point(false);
        let public_bytes = public.as_bytes();
        let public_key = Base64UrlUnpadded::encode_string(public_bytes);
        let digest = Sha256::digest(public_bytes);
        let key_id = format!("vpk_{}", &hex(&digest)[..12]);
        return Ok(StoredVapidKey {
            version: VERSION,
            key_id,
            private_key: Base64UrlUnpadded::encode_string(&bytes),
            public_key,
            subject: subject.to_string(),
            created_at: now,
        });
    }
    Err(VapidKeyError::Generation)
}

fn validate(path: &Path, subject: &str, stored: &StoredVapidKey) -> Result<(), VapidKeyError> {
    if stored.version != VERSION {
        return Err(invalid(path, "unsupported file version"));
    }
    if stored.subject != subject {
        return Err(invalid(path, "subject does not match the control origin"));
    }
    let private = Base64UrlUnpadded::decode_vec(&stored.private_key)
        .map_err(|_| invalid(path, "private key base64 is invalid"))?;
    if private.len() != 32 {
        return Err(invalid(path, "private key must contain 32 bytes"));
    }
    let secret = SecretKey::from_slice(&private)
        .map_err(|_| invalid(path, "private key scalar is invalid"))?;
    let public = secret.public_key().to_encoded_point(false);
    if Base64UrlUnpadded::encode_string(public.as_bytes()) != stored.public_key {
        return Err(invalid(path, "public key does not match private key"));
    }
    let digest = Sha256::digest(public.as_bytes());
    if stored.key_id != format!("vpk_{}", &hex(&digest)[..12]) {
        return Err(invalid(path, "key_id does not match public key"));
    }
    ES256KeyPair::from_bytes(&private)
        .map_err(|_| invalid(path, "private key is not valid for VAPID"))?;
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn invalid(path: &Path, reason: &'static str) -> VapidKeyError {
    VapidKeyError::Invalid {
        path: path.to_path_buf(),
        reason,
    }
}

fn file_error(operation: &'static str, path: &Path, reason: String) -> VapidKeyError {
    VapidKeyError::File {
        operation,
        path: path.to_path_buf(),
        reason,
    }
}
