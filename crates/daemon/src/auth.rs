//! Machine-token and single-user browser authentication.

mod password;

pub use password::{AuthManager, CredentialKind};

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use protocol::Timestamp;

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
    /// The bootstrap machine token was missing or wrong.
    #[error("invalid bootstrap machine token")]
    InvalidBootstrap,
    /// The password does not meet the daemon's validation rules.
    #[error("invalid password: {reason}")]
    InvalidPassword {
        /// Validation detail safe to show to the owner.
        reason: String,
    },
    /// Login was attempted before the owner configured a password.
    #[error("owner password setup is required")]
    SetupRequired,
    /// Setup was attempted after a password was already configured.
    #[error("owner password is already configured")]
    AlreadyConfigured,
    /// The supplied password was incorrect.
    #[error("invalid owner password; {attempts_remaining} attempts remaining")]
    InvalidCredentials {
        /// Attempts left before this client is locked.
        attempts_remaining: u32,
    },
    /// This client made too many failed login attempts.
    #[error("password login is locked until {retry_at:?}")]
    Locked {
        /// Lockout end in Unix milliseconds.
        retry_at: Timestamp,
    },
    /// A browser session token was missing, invalid, or expired.
    #[error("invalid or expired browser session")]
    InvalidSession,
    /// A password-file operation failed.
    #[error("failed to {operation} password file '{}': {source}", path.display())]
    PasswordFile {
        /// Operation that failed.
        operation: &'static str,
        /// Password file path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Stored password data was malformed.
    #[error("invalid password file '{}': {message}", path.display())]
    PasswordData {
        /// Password file path.
        path: PathBuf,
        /// Parse or hash detail.
        message: String,
    },
    /// Argon2 could not hash or verify a password.
    #[error("password hashing failed: {0}")]
    PasswordHash(String),
    /// A password hashing worker could not complete.
    #[error("password hashing worker failed: {0}")]
    HashTask(String),
    /// Shared authentication state could not be accessed.
    #[error("authentication state lock is poisoned")]
    StatePoisoned,
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

    let token = random_hex_token()?;

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

pub(crate) fn secure_random<const N: usize>() -> Result<[u8; N], AuthError> {
    let mut random = [0_u8; N];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut random))
        .map_err(AuthError::Random)?;
    Ok(random)
}

pub(crate) fn random_hex_token() -> Result<String, AuthError> {
    Ok(secure_random::<TOKEN_BYTES>()?
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
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
