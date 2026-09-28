//! Choosing the rc file and applying block edits safely on disk.

use crate::{
    hook_script::{ensure_not_world_writable, HookScriptError, HookShell},
    rc_file::RcError,
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

/// Facts used to pick an rc file, injected so selection is testable.
#[derive(Debug, Clone)]
pub struct RcEnv {
    /// User home directory.
    pub home: PathBuf,
    /// `$SHELL`, used when no shell is named.
    pub shell: Option<String>,
    /// `$ZDOTDIR`.
    pub zdotdir: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`.
    pub xdg_config_home: Option<PathBuf>,
    /// Whether this host is macOS (bash login-shell conventions differ).
    pub macos: bool,
}

/// Why installing or uninstalling did not change anything.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    /// The shell could not be determined.
    #[error("cannot tell which shell to configure from $SHELL ({}); pass zsh, bash, or fish", .0.as_deref().unwrap_or("unset"))]
    UnknownShell(Option<String>),
    /// More than one rc file could be the right one.
    #[error("{0}; pass --rc PATH to choose")]
    Ambiguous(String),
    /// The block edit was refused.
    #[error("{}: {source}", path.display())]
    Rc {
        /// File being edited.
        path: PathBuf,
        /// Why the edit was refused.
        source: RcError,
    },
    /// An unsafe path or binary was refused.
    #[error(transparent)]
    Unsafe(#[from] HookScriptError),
    /// A filesystem operation failed; the original file is unchanged.
    #[error("failed to {operation} '{}': {source}", path.display())]
    Io {
        /// Operation being attempted.
        operation: &'static str,
        /// Path involved.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
}

/// Result of an install or uninstall.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// The file already had the requested content; nothing was written.
    Unchanged,
    /// The file was replaced; `backup` holds the previous bytes, if there were any.
    Written {
        /// Owner-only copy of the previous file.
        backup: Option<PathBuf>,
    },
}

/// Pick the shell and rc file, refusing to guess when it is ambiguous.
pub fn select_rc(
    shell: Option<HookShell>,
    explicit: Option<PathBuf>,
    env: &RcEnv,
) -> Result<(HookShell, PathBuf), InstallError> {
    let shell = match shell {
        Some(shell) => shell,
        None => env
            .shell
            .as_deref()
            .and_then(HookShell::from_shell_path)
            .ok_or_else(|| InstallError::UnknownShell(env.shell.clone()))?,
    };
    if let Some(path) = explicit {
        return Ok((shell, path));
    }
    let path = match shell {
        HookShell::Zsh => env
            .zdotdir
            .clone()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or_else(|| env.home.clone())
            .join(".zshrc"),
        HookShell::Fish => env
            .xdg_config_home
            .clone()
            .filter(|dir| dir.is_absolute())
            .unwrap_or_else(|| env.home.join(".config"))
            .join("fish/config.fish"),
        HookShell::Bash if !env.macos => env.home.join(".bashrc"),
        HookShell::Bash => macos_bash_rc(&env.home)?,
    };
    Ok((shell, path))
}

/// macOS terminals start bash as a login shell, which reads only the first
/// of `.bash_profile`, `.bash_login`, `.profile`.
fn macos_bash_rc(home: &Path) -> Result<PathBuf, InstallError> {
    let first = [".bash_profile", ".bash_login", ".profile"]
        .into_iter()
        .map(|name| home.join(name))
        .find(|path| path.exists());
    match first {
        None => Ok(home.join(".bash_profile")),
        Some(path) if path.ends_with(".profile") => Err(InstallError::Ambiguous(format!(
            "bash login shells here read {}, which /bin/sh also reads",
            path.display()
        ))),
        Some(path) => Ok(path),
    }
}

/// Apply `edit` to the rc file at `path` atomically, with a backup first.
pub fn apply(
    path: &Path,
    now_ms: i64,
    edit: impl FnOnce(&[u8]) -> Result<Vec<u8>, RcError>,
) -> Result<Applied, InstallError> {
    apply_with_rename(path, now_ms, edit, |from, to| fs::rename(from, to))
}

/// [`apply`] with an injectable final rename, for failure testing.
pub fn apply_with_rename(
    path: &Path,
    now_ms: i64,
    edit: impl FnOnce(&[u8]) -> Result<Vec<u8>, RcError>,
    rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
) -> Result<Applied, InstallError> {
    // Edit a symlink's target so dotfile-manager links survive.
    let target = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => fs::canonicalize(path)
            .map_err(|source| io("resolve symlinked rc file", path, source))?,
        _ => path.to_path_buf(),
    };
    let (original, mode) = match fs::read(&target) {
        Ok(bytes) => {
            let mode = fs::metadata(&target)
                .map_err(|source| io("inspect", &target, source))?
                .permissions()
                .mode()
                & 0o7777;
            (Some(bytes), mode)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, 0o644),
        Err(source) => return Err(io("read", &target, source)),
    };
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf();
    let updated =
        edit(original.as_deref().unwrap_or_default()).map_err(|source| InstallError::Rc {
            path: target.clone(),
            source,
        })?;
    if Some(updated.as_slice()) == original.as_deref() || (original.is_none() && updated.is_empty())
    {
        return Ok(Applied::Unchanged);
    }

    fs::create_dir_all(&parent).map_err(|source| io("create directory", &parent, source))?;
    ensure_not_world_writable(&parent)?;
    if original.is_some() {
        ensure_not_world_writable(&target)?;
    }
    let backup = match &original {
        Some(bytes) => Some(write_backup(&target, bytes, now_ms)?),
        None => None,
    };

    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "rc".into());
    let temp = parent.join(format!(".{name}.sentinal-tmp-{}", std::process::id()));
    let written = write_new_file(&temp, &updated, mode)
        .and_then(|()| rename(&temp, &target).map_err(|source| io("replace", &target, source)));
    if let Err(error) = written {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    Ok(Applied::Written { backup })
}

fn write_backup(target: &Path, bytes: &[u8], now_ms: i64) -> Result<PathBuf, InstallError> {
    let base = target.as_os_str().to_string_lossy().into_owned();
    for attempt in 0..100 {
        let suffix = if attempt == 0 {
            String::new()
        } else {
            format!("-{attempt}")
        };
        let backup = PathBuf::from(format!("{base}.sentinal-backup-{now_ms}{suffix}"));
        match write_new_file(&backup, bytes, 0o600) {
            Ok(()) => return Ok(backup),
            Err(InstallError::Io { source, .. })
                if source.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io(
        "create backup of",
        target,
        std::io::Error::other("too many backups with the same timestamp"),
    ))
}

/// Create `path` exclusively with exactly `mode`, write, and flush to disk.
fn write_new_file(path: &Path, bytes: &[u8], mode: u32) -> Result<(), InstallError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(mode)
        .open(path)
        .map_err(|source| io("create", path, source))?;
    file.write_all(bytes)
        .and_then(|()| file.set_permissions(fs::Permissions::from_mode(mode)))
        .and_then(|()| file.sync_all())
        .map_err(|source| io("write", path, source))
}

fn io(operation: &'static str, path: &Path, source: std::io::Error) -> InstallError {
    InstallError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}
