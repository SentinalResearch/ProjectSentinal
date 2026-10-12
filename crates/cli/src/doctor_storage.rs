//! The doctor `Storage:` line (F9): what Session logs, audit files and the
//! database use on disk, against the `[storage]` limits. Read-only; it only
//! looks at file sizes, never contents.

use std::path::{Path, PathBuf};

const MIB: f64 = 1024.0 * 1024.0;
const DEFAULT_LOG_MAX_BYTES: u64 = 4 * 1024 * 1024 * 1024;
const DEFAULT_LOG_MAX_AGE_DAYS: u64 = 90;

/// One line describing storage use, or why it could not be measured.
pub fn describe(daemon_toml: &Path, home: &Path) -> String {
    let value: Option<toml::Value> = std::fs::read_to_string(daemon_toml)
        .ok()
        .and_then(|text| toml::from_str(&text).ok());
    let data_dir = value
        .as_ref()
        .and_then(|value| value.get("data_dir"))
        .and_then(toml::Value::as_str)
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(protocol::local::DEFAULT_DATA_DIR));
    let storage = value.as_ref().and_then(|value| value.get("storage"));
    let limit = |key: &str, default: u64| {
        storage
            .and_then(|storage| storage.get(key))
            .and_then(toml::Value::as_integer)
            .and_then(|value| u64::try_from(value).ok())
            .unwrap_or(default)
    };
    let max_bytes = limit("session_log_max_bytes", DEFAULT_LOG_MAX_BYTES);
    let max_days = limit("session_log_max_age_days", DEFAULT_LOG_MAX_AGE_DAYS);
    if !data_dir.is_dir() {
        return format!("no data directory yet ({})", data_dir.display());
    }

    let (log_bytes, log_count) = files_in(&data_dir.join("sessions"))
        .filter(|(name, _)| name.ends_with(".log.zst"))
        .fold((0, 0), |(bytes, count), (_, len)| (bytes + len, count + 1));
    let mut audit_bytes = audit_bytes_in(&data_dir);
    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && entry.file_name() != "sessions" {
                audit_bytes += audit_bytes_in(&path);
            }
        }
    }
    let database_bytes: u64 = files_in(&data_dir)
        .filter(|(name, _)| name.starts_with("terminal-agent.sqlite3"))
        .map(|(_, len)| len)
        .sum();
    format!(
        "Session logs {} ({log_count}) of {:.0} MiB, kept {max_days} days · audits {} · database {}",
        mib(log_bytes),
        max_bytes as f64 / MIB,
        mib(audit_bytes),
        mib(database_bytes)
    )
}

fn audit_bytes_in(dir: &Path) -> u64 {
    files_in(dir)
        .filter(|(name, _)| name.contains(".jsonl"))
        .map(|(_, len)| len)
        .sum()
}

/// Regular files directly in `dir` (symlinks are not followed), as (name, size).
fn files_in(dir: &Path) -> impl Iterator<Item = (String, u64)> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let metadata = std::fs::symlink_metadata(entry.path()).ok()?;
            metadata.is_file().then(|| {
                (
                    entry.file_name().to_string_lossy().into_owned(),
                    metadata.len(),
                )
            })
        })
}

fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / MIB)
}
