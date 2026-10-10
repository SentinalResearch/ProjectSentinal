//! Verified worker updates through the installed Tailnet package origin.

use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_INSTALL_SCRIPT_BYTES: u64 = 64 * 1024;

/// Failures while checking or installing a worker release.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    /// The installation prefix could not be inferred.
    #[error("cannot locate the Sentinal install prefix; pass --prefix explicitly")]
    MissingPrefix,
    /// An installation path was unsafe.
    #[error("unsafe update {name} path '{path}': expected an absolute, non-root path")]
    UnsafePath {
        /// Path role.
        name: &'static str,
        /// Rejected path.
        path: PathBuf,
    },
    /// Release metadata could not be read.
    #[error("failed to read installed release metadata '{}': {source}", path.display())]
    ReadState {
        /// Metadata path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Release metadata was invalid.
    #[error("invalid installed release metadata '{}': {reason}", path.display())]
    InvalidState {
        /// Metadata path.
        path: PathBuf,
        /// Validation failure.
        reason: String,
    },
    /// The installed package origin was unsafe.
    #[error("invalid update origin '{0}'; expected one credential-free HTTPS .ts.net origin")]
    UnsafeOrigin(String),
    /// The requested rollback version was malformed.
    #[error("invalid rollback version '{0}'; expected MAJOR.MINOR.PATCH")]
    InvalidVersion(String),
    /// A temporary directory operation failed.
    #[error("failed to prepare update temporary path '{}': {source}", path.display())]
    Temporary {
        /// Temporary path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Curl could not be launched.
    #[error("failed to start curl for '{url}/install.sh': {source}")]
    CurlStart {
        /// Package origin.
        url: String,
        /// Process error.
        source: std::io::Error,
    },
    /// Curl rejected or could not download the script.
    #[error("failed to download verified installer from '{url}/install.sh' (curl {status})")]
    CurlFailed {
        /// Package origin.
        url: String,
        /// Curl exit status.
        status: std::process::ExitStatus,
    },
    /// The downloaded script exceeded its bound.
    #[error("downloaded install script exceeds 65536 bytes")]
    ScriptTooLarge,
    /// Script metadata could not be read.
    #[error("failed to inspect downloaded install script '{}': {source}", path.display())]
    ScriptMetadata {
        /// Script path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// The verified bootstrap could not be launched.
    #[error("failed to start verified install script '{}': {source}", path.display())]
    ScriptStart {
        /// Script path.
        path: PathBuf,
        /// Process error.
        source: std::io::Error,
    },
    /// The installed release records no control to update from.
    #[error(
        "this installation has no update origin ('{}' names no control_url). On the control, \
         publish new releases with `just receive-release`; on a worker, reinstall once from \
         its control's https://…ts.net/install.sh",
        path.display()
    )]
    NoOrigin {
        /// Release metadata path.
        path: PathBuf,
    },
    /// The verified bootstrap returned failure.
    #[error("verified install script failed with {0}")]
    ScriptFailed(std::process::ExitStatus),
}

/// Reuse the installed Tailnet origin to check, update, or roll back.
pub fn run(
    check: bool,
    rollback: Option<String>,
    prefix: Option<PathBuf>,
    out: &mut dyn std::io::Write,
) -> Result<ExitCode, UpdateError> {
    let _ = writeln!(out, "sentinal {}", crate::version::VERSION.as_str());
    let prefix = match prefix {
        Some(prefix) => prefix,
        None => installed_prefix()?,
    };
    require_safe_path("prefix", &prefix)?;
    if let Some(version) = rollback.as_deref() {
        if !valid_version(version) {
            return Err(UpdateError::InvalidVersion(version.to_string()));
        }
    }
    let state_path = prefix.join("share/sentinal/release.json");
    let origin = read_origin(&state_path)?;
    validate_origin(&origin)?;

    let temporary = TemporaryDirectory::create()?;
    let script = temporary.path.join("install.sh");
    let status = Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-redirs",
            "0",
            "--max-filesize",
            "65536",
            "--fail",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&script)
        .arg(format!("{origin}/install.sh"))
        .status()
        .map_err(|source| UpdateError::CurlStart {
            url: origin.clone(),
            source,
        })?;
    if !status.success() {
        return Err(UpdateError::CurlFailed {
            url: origin,
            status,
        });
    }
    let metadata = fs::metadata(&script).map_err(|source| UpdateError::ScriptMetadata {
        path: script.clone(),
        source,
    })?;
    if !metadata.is_file() || metadata.len() > MAX_INSTALL_SCRIPT_BYTES {
        return Err(UpdateError::ScriptTooLarge);
    }
    let mut command = Command::new("sh");
    command.arg(&script);
    if check {
        command.arg("--check");
    } else if let Some(version) = rollback {
        command.args(["--rollback", &version]);
    }
    command.arg("--prefix").arg(&prefix);
    let status = command
        .status()
        .map_err(|source| UpdateError::ScriptStart {
            path: script,
            source,
        })?;
    if !status.success() {
        return Err(UpdateError::ScriptFailed(status));
    }
    Ok(ExitCode::SUCCESS)
}

fn installed_prefix() -> Result<PathBuf, UpdateError> {
    let executable = std::env::current_exe().map_err(|_| UpdateError::MissingPrefix)?;
    executable
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or(UpdateError::MissingPrefix)
}

fn read_origin(path: &Path) -> Result<String, UpdateError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| UpdateError::ReadState {
        path: path.to_path_buf(),
        source,
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 64 * 1024 {
        return Err(UpdateError::InvalidState {
            path: path.to_path_buf(),
            reason: "expected one regular file no larger than 65536 bytes".into(),
        });
    }
    let bytes = fs::read(path).map_err(|source| UpdateError::ReadState {
        path: path.to_path_buf(),
        source,
    })?;
    let state: Value =
        serde_json::from_slice(&bytes).map_err(|error| UpdateError::InvalidState {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
    state
        .get("control_url")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or_else(|| UpdateError::NoOrigin {
            path: path.to_path_buf(),
        })
}

fn validate_origin(value: &str) -> Result<(), UpdateError> {
    let Some(host) = value.strip_prefix("https://") else {
        return Err(UpdateError::UnsafeOrigin(value.to_string()));
    };
    if host.is_empty()
        || !host.ends_with(".ts.net")
        || host.contains(['/', '?', '#', '@', ':'])
        || !host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
    {
        return Err(UpdateError::UnsafeOrigin(value.to_string()));
    }
    Ok(())
}

fn valid_version(value: &str) -> bool {
    let mut parts = value.split('.');
    let part = |value: &str| {
        !value.is_empty()
            && value.len() <= 9
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && (value == "0" || !value.starts_with('0'))
    };
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(major), Some(minor), Some(patch), None)
            if part(major) && part(minor) && part(patch)
    )
}

fn require_safe_path(name: &'static str, path: &Path) -> Result<(), UpdateError> {
    if !path.is_absolute() || path.parent().is_none() || path == Path::new("/") {
        return Err(UpdateError::UnsafePath {
            name,
            path: path.to_path_buf(),
        });
    }
    Ok(())
}

struct TemporaryDirectory {
    path: PathBuf,
}

impl TemporaryDirectory {
    fn create() -> Result<Self, UpdateError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("sentinal-update-{}-{now}", std::process::id()));
        fs::create_dir(&path).map_err(|source| UpdateError::Temporary {
            path: path.clone(),
            source,
        })?;
        set_owner_only(&path).map_err(|source| UpdateError::Temporary {
            path: path.clone(),
            source,
        })?;
        Ok(Self { path })
    }
}

impl Drop for TemporaryDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn set_owner_only(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn set_owner_only(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}
