//! Control-side secrets kept out of SQLite: the pairing HMAC key and the
//! credentials the control presents to each worker.

use crate::{link_crypto, private_fs::private_file_options};
use protocol::{MachineId, Secret, Timestamp};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

const VERSION: u32 = 1;

/// One credential the control presents to a worker.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PresentedCredential {
    pub(super) machine_id: MachineId,
    pub(super) control_credential: Secret,
}

/// A credential refresh between its two writes (F5): what to restore if it
/// does not complete. Written atomically together with the new credential.
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PendingRefresh {
    pub(super) link_id: String,
    /// The credential presented before the refresh (`None`: there was none).
    pub(super) previous: Option<PresentedCredential>,
    /// SHA-256 of the worker credential before the refresh.
    pub(super) previous_sha256: String,
    /// Expiry of the worker credential before the refresh.
    pub(super) previous_expires_at: Timestamp,
}

#[derive(Serialize, Deserialize)]
struct SecretsFile {
    version: u32,
    pairing_key: Secret,
    links: BTreeMap<String, PresentedCredential>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_refresh: Option<PendingRefresh>,
}

/// In-memory copy of the owner-only secrets file.
pub(super) struct ControlSecrets {
    path: PathBuf,
    file: SecretsFile,
}

impl ControlSecrets {
    /// Load the file, creating it with a new pairing key on first use.
    pub(super) fn load(path: &Path) -> Result<Self, String> {
        let file = match fs::read_to_string(path) {
            Ok(text) => {
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
                let file: SecretsFile = serde_json::from_str(&text)
                    .map_err(|error| format!("'{}' is invalid: {error}", path.display()))?;
                if file.version != VERSION
                    || link_crypto::from_hex(file.pairing_key.expose()).is_none()
                {
                    return Err(format!("'{}' has an unsupported format", path.display()));
                }
                file
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => SecretsFile {
                version: VERSION,
                pairing_key: Secret::new(
                    link_crypto::new_credential().map_err(|error| error.to_string())?,
                ),
                links: BTreeMap::new(),
                pending_refresh: None,
            },
            Err(error) => return Err(format!("cannot read '{}': {error}", path.display())),
        };
        let secrets = ControlSecrets {
            path: path.to_path_buf(),
            file,
        };
        secrets.persist()?;
        Ok(secrets)
    }

    /// HMAC key for pairing codes.
    pub(super) fn pairing_key(&self) -> Vec<u8> {
        link_crypto::from_hex(self.file.pairing_key.expose()).unwrap_or_default()
    }

    /// Store the credential presented to the worker of `link_id`.
    pub(super) fn insert(
        &mut self,
        link_id: &str,
        credential: PresentedCredential,
    ) -> Result<(), String> {
        self.file.links.insert(link_id.to_string(), credential);
        self.persist()
    }

    /// Present `credential` for `link_id`, recording how to undo it, in one write.
    pub(super) fn begin_refresh(
        &mut self,
        credential: PresentedCredential,
        pending: PendingRefresh,
    ) -> Result<(), String> {
        self.file.links.insert(pending.link_id.clone(), credential);
        self.file.pending_refresh = Some(pending);
        self.persist()
    }

    /// The refresh completed on both sides; forget how to undo it.
    pub(super) fn commit_refresh(&mut self) -> Result<(), String> {
        self.file.pending_refresh = None;
        self.persist()
    }

    /// A refresh that did not complete, if any.
    pub(super) fn pending_refresh(&self) -> Option<PendingRefresh> {
        self.file.pending_refresh.clone()
    }

    /// Restore the credential presented before an incomplete refresh.
    pub(super) fn abort_refresh(&mut self) -> Result<(), String> {
        if let Some(pending) = self.file.pending_refresh.take() {
            match pending.previous {
                Some(previous) => {
                    self.file.links.insert(pending.link_id, previous);
                }
                None => {
                    self.file.links.remove(&pending.link_id);
                }
            }
        }
        self.persist()
    }

    /// Forget the credentials of `link_ids`.
    pub(super) fn remove(&mut self, link_ids: &[String]) -> Result<(), String> {
        for link_id in link_ids {
            self.file.links.remove(link_id);
        }
        self.persist()
    }

    /// The credential presented to a worker's current link.
    pub(super) fn credential(&self, link_id: &str) -> Option<&PresentedCredential> {
        self.file.links.get(link_id)
    }

    fn persist(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec(&self.file).map_err(|error| error.to_string())?;
        let suffix = link_crypto::new_handle("tmp").map_err(|error| error.to_string())?;
        let temporary = self.path.with_extension(suffix);
        let written = private_file_options()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .and_then(|mut file| file.write_all(&bytes).and_then(|()| file.sync_all()))
            .and_then(|()| fs::rename(&temporary, &self.path));
        if let Err(error) = written {
            let _ = fs::remove_file(&temporary);
            return Err(format!("cannot write '{}': {error}", self.path.display()));
        }
        Ok(())
    }
}
