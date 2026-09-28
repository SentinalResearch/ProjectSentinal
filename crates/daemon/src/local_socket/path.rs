//! Socket location selection and filesystem safety checks.

use super::{LocalSocketError, MAX_SOCKET_PATH_BYTES};
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// Select the explicit path, Linux runtime path, or data-directory fallback.
pub fn default_socket_path(
    data_dir: &Path,
    explicit: Option<&Path>,
    xdg_runtime_dir: Option<&Path>,
    target_is_macos: bool,
) -> PathBuf {
    match explicit {
        Some(explicit) => explicit.to_path_buf(),
        None => protocol::local::local_socket_path(data_dir, xdg_runtime_dir, target_is_macos),
    }
}

pub(super) fn validate_socket_length(path: &Path) -> Result<(), LocalSocketError> {
    if path.as_os_str().as_encoded_bytes().len() > MAX_SOCKET_PATH_BYTES {
        Err(LocalSocketError::PathTooLong {
            path: path.to_path_buf(),
        })
    } else {
        Ok(())
    }
}

/// Create or verify the socket's parent as a real directory owned by
/// `expected_uid` with mode `0700`.
pub(super) fn prepare_private_parent(
    parent: &Path,
    expected_uid: u32,
) -> Result<(), LocalSocketError> {
    validate_ancestors(parent, expected_uid)?;
    let base = parent
        .parent()
        .ok_or_else(|| LocalSocketError::MissingParent(parent.to_path_buf()))?;
    let base_metadata = fs::symlink_metadata(base).map_err(|source| LocalSocketError::Io {
        operation: "inspect socket base directory for",
        path: parent.to_path_buf(),
        source,
    })?;
    if base_metadata.file_type().is_symlink() || !base_metadata.is_dir() {
        return Err(LocalSocketError::UnsafePath {
            path: base.to_path_buf(),
            reason: "base directory is a symlink or is not a directory".into(),
        });
    }
    match fs::symlink_metadata(parent) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(LocalSocketError::UnsafePath {
                    path: parent.to_path_buf(),
                    reason: "parent is a symlink or is not a directory".into(),
                });
            }
            if metadata.uid() != expected_uid {
                return Err(LocalSocketError::UnsafePath {
                    path: parent.to_path_buf(),
                    reason: format!(
                        "parent uid {} does not match expected uid {expected_uid}",
                        metadata.uid()
                    ),
                });
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(parent).map_err(|source| LocalSocketError::Io {
                operation: "create private parent for",
                path: parent.to_path_buf(),
                source,
            })?;
        }
        Err(source) => {
            return Err(LocalSocketError::Io {
                operation: "inspect private parent for",
                path: parent.to_path_buf(),
                source,
            });
        }
    }
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).map_err(|source| {
        LocalSocketError::Io {
            operation: "set parent mode 0700 for",
            path: parent.to_path_buf(),
            source,
        }
    })?;
    let metadata = fs::symlink_metadata(parent).map_err(|source| LocalSocketError::Io {
        operation: "verify private parent for",
        path: parent.to_path_buf(),
        source,
    })?;
    if metadata.uid() != expected_uid || metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(LocalSocketError::UnsafePath {
            path: parent.to_path_buf(),
            reason: "parent ownership or mode changed during setup".into(),
        });
    }
    Ok(())
}

fn validate_ancestors(path: &Path, expected_uid: u32) -> Result<(), LocalSocketError> {
    if !path.is_absolute() {
        return Err(LocalSocketError::UnsafePath {
            path: path.to_path_buf(),
            reason: "socket parent must be an absolute path".into(),
        });
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(source) => {
                return Err(LocalSocketError::Io {
                    operation: "inspect socket ancestor for",
                    path: current,
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() && trusted_system_alias(&current) {
            continue;
        }
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(LocalSocketError::UnsafePath {
                path: current,
                reason: "ancestor is a symlink or is not a directory".into(),
            });
        }
        let mode = metadata.permissions().mode();
        let writable_by_others = mode & 0o022 != 0;
        let sticky = mode & 0o1000 != 0;
        if writable_by_others && (metadata.uid() == expected_uid || !sticky) {
            return Err(LocalSocketError::UnsafePath {
                path: current,
                reason: "ancestor is group/world writable without safe sticky ownership".into(),
            });
        }
    }
    Ok(())
}

fn trusted_system_alias(path: &Path) -> bool {
    #[cfg(target_os = "macos")]
    {
        let expected = if path == Path::new("/var") {
            Some(Path::new("private/var"))
        } else if path == Path::new("/tmp") {
            Some(Path::new("private/tmp"))
        } else {
            None
        };
        expected.is_some_and(|target| fs::read_link(path).ok().as_deref() == Some(target))
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        false
    }
}

/// Refuse a symlink, non-socket, foreign-owned, or live socket; remove a
/// verified stale one owned by `expected_uid`.
pub fn inspect_existing_socket(path: &Path, expected_uid: u32) -> Result<(), LocalSocketError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => {
            return Err(LocalSocketError::Io {
                operation: "inspect existing",
                path: path.to_path_buf(),
                source,
            });
        }
    };
    if metadata.file_type().is_symlink() {
        return Err(LocalSocketError::UnsafePath {
            path: path.to_path_buf(),
            reason: "socket path is a symlink".into(),
        });
    }
    if !metadata.file_type().is_socket() {
        return Err(LocalSocketError::UnsafePath {
            path: path.to_path_buf(),
            reason: "existing path is not a Unix socket".into(),
        });
    }
    if metadata.uid() != expected_uid {
        return Err(LocalSocketError::UnsafePath {
            path: path.to_path_buf(),
            reason: format!(
                "socket uid {} does not match expected uid {expected_uid}",
                metadata.uid()
            ),
        });
    }
    match std::os::unix::net::UnixStream::connect(path) {
        Ok(_) => Err(LocalSocketError::UnsafePath {
            path: path.to_path_buf(),
            reason: "another daemon is accepting connections".into(),
        }),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            fs::remove_file(path).map_err(|source| LocalSocketError::Io {
                operation: "remove verified stale",
                path: path.to_path_buf(),
                source,
            })
        }
        Err(source) => Err(LocalSocketError::Io {
            operation: "probe existing",
            path: path.to_path_buf(),
            source,
        }),
    }
}
