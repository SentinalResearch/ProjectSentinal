//! Owner-only directories and files for daemon data.
//!
//! The data directory holds terminal output, Command history and audit
//! metadata, so nothing in it may be readable by other local users regardless
//! of the process umask.

use std::{
    fs::{self, OpenOptions},
    io,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// Mode for daemon data directories.
pub const PRIVATE_DIR_MODE: u32 = 0o700;
/// Mode for daemon data files.
pub const PRIVATE_FILE_MODE: u32 = 0o600;

/// Create `path` if needed and restrict it to the owner, tightening an existing directory.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    let metadata = fs::metadata(path)?;
    if !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "data path is not a directory",
        ));
    }
    if metadata.permissions().mode() & 0o777 != PRIVATE_DIR_MODE {
        fs::set_permissions(path, fs::Permissions::from_mode(PRIVATE_DIR_MODE))?;
    }
    let metadata = fs::metadata(path)?;
    if metadata.permissions().mode() & 0o077 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "directory mode is {:o}, expected {PRIVATE_DIR_MODE:o}",
                metadata.permissions().mode() & 0o777
            ),
        ));
    }
    Ok(())
}

/// Options that create a new file as owner read/write only.
pub fn private_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.mode(PRIVATE_FILE_MODE);
    options
}
