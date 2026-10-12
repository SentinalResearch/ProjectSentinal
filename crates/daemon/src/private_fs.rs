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

/// Largest a JSONL audit file grows before it is rotated (F9).
pub const AUDIT_ROTATE_BYTES: u64 = 16 * 1024 * 1024;
/// Rotated generations kept beside the live file (`.1` newest … `.3` oldest).
pub const AUDIT_KEEP_ROTATED: u32 = 3;

/// Append `line` to an owner-only JSONL file, first rotating it when the
/// append would pass `max_bytes`: `name.jsonl` → `name.jsonl.1` → … and the
/// oldest beyond `keep` is deleted. Each file is bounded, so the total is
/// bounded at `(keep + 1) × max_bytes`.
pub fn append_bounded(path: &Path, line: &[u8], max_bytes: u64, keep: u32) -> io::Result<()> {
    use std::io::Write;
    let size = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata.len(),
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "audit path is not a regular file",
            ))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error),
    };
    if size > 0 && size.saturating_add(line.len() as u64) > max_bytes {
        rotate(path, keep)?;
    }
    private_file_options()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(line)
}

fn rotated(path: &Path, generation: u32) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".{generation}"));
    name.into()
}

fn rotate(path: &Path, keep: u32) -> io::Result<()> {
    if keep == 0 {
        return fs::remove_file(path);
    }
    let oldest = rotated(path, keep);
    if oldest.exists() {
        fs::remove_file(&oldest)?;
    }
    for generation in (1..keep).rev() {
        let from = rotated(path, generation);
        if from.exists() {
            fs::rename(&from, rotated(path, generation + 1))?;
        }
    }
    fs::rename(path, rotated(path, 1))
}

#[cfg(test)]
mod tests {
    use super::{append_bounded, rotated};
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn audit_files_rotate_and_keep_a_bounded_number_of_generations() {
        let dir = std::env::temp_dir().join(format!("sentinal-rotate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("audit.jsonl");
        for index in 0..20 {
            append_bounded(&path, format!("{{\"n\":{index:04}}}\n").as_bytes(), 40, 2).unwrap();
        }
        let live = fs::read_to_string(&path).unwrap();
        assert!(live.len() <= 40, "live file is bounded: {live:?}");
        assert!(
            live.ends_with("{\"n\":0019}\n"),
            "newest record is in the live file"
        );
        assert!(rotated(&path, 1).exists() && rotated(&path, 2).exists());
        assert!(!rotated(&path, 3).exists(), "older generations are deleted");
        for file in [path.clone(), rotated(&path, 1), rotated(&path, 2)] {
            assert_eq!(
                fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        let total: u64 = [path.clone(), rotated(&path, 1), rotated(&path, 2)]
            .iter()
            .map(|file| fs::metadata(file).unwrap().len())
            .sum();
        assert!(total <= 3 * 40);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unwritable_audit_path_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("sentinal-rotate-err-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("audit.jsonl")).unwrap(); // a directory where the file should be
        assert!(append_bounded(&dir.join("audit.jsonl"), b"{}\n", 40, 2).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
