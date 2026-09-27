//! Bearer token creation and comparison.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

const TOKEN_BYTES: usize = 32;

/// Errors produced while loading or creating a daemon token.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// A token filesystem operation failed.
    #[error("failed to {operation} token file '{}': {source}", path.display())]
    File {
        /// Operation that failed.
        operation: &'static str,
        /// Token file path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// The token file contained no token.
    #[error("token file '{}' is empty", .0.display())]
    Empty(PathBuf),
    /// Secure random bytes could not be read.
    #[error("failed to read secure random bytes from /dev/urandom: {0}")]
    Random(std::io::Error),
}

/// Load an existing token or create a new 256-bit token with mode 0600.
pub fn load_or_create_token(path: &Path) -> Result<String, AuthError> {
    match fs::read_to_string(path) {
        Ok(token) => validated_token(path, token),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => create_token(path),
        Err(source) => Err(AuthError::File {
            operation: "read",
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn validated_token(path: &Path, token: String) -> Result<String, AuthError> {
    let token = token.trim().to_string();
    if token.is_empty() {
        Err(AuthError::Empty(path.to_path_buf()))
    } else {
        Ok(token)
    }
}

fn create_token(path: &Path) -> Result<String, AuthError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| AuthError::File {
            operation: "create parent directory for",
            path: path.to_path_buf(),
            source,
        })?;
    }

    let mut random = [0_u8; TOKEN_BYTES];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(AuthError::Random)?;
    let token = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    match options.open(path) {
        Ok(mut file) => file
            .write_all(token.as_bytes())
            .map_err(|source| AuthError::File {
                operation: "write",
                path: path.to_path_buf(),
                source,
            })?,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return load_or_create_token(path);
        }
        Err(source) => {
            return Err(AuthError::File {
                operation: "create",
                path: path.to_path_buf(),
                source,
            });
        }
    }
    Ok(token)
}

/// Compare bearer tokens without returning early on mismatched bytes or lengths.
pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        let left_byte = left.get(index).copied().unwrap_or(0);
        let right_byte = right.get(index).copied().unwrap_or(0);
        difference |= usize::from(left_byte ^ right_byte);
    }
    difference == 0
}
