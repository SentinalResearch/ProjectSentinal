//! Bounded filesystem loading for pure declarative watcher adapters.

use std::path::{Path, PathBuf};
use watcher::{DeclarativeAdapter, DeclarativeError};

const MAX_ADAPTER_BYTES: u64 = 1024 * 1024;

/// Failure reading or compiling a declarative watcher adapter.
#[derive(Debug, thiserror::Error)]
pub enum AdapterConfigError {
    /// The adapter directory could not be listed.
    #[error("failed to list watcher adapter directory '{}': {source}", path.display())]
    Directory {
        /// Adapter directory.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// One adapter file could not be inspected or read.
    #[error("failed to {operation} watcher adapter '{}': {source}", path.display())]
    File {
        /// Operation that failed.
        operation: &'static str,
        /// Adapter file.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// One adapter exceeded the bounded configuration size.
    #[error("watcher adapter '{}' is {actual} bytes; maximum is {maximum}", path.display())]
    TooLarge {
        /// Adapter file.
        path: PathBuf,
        /// Actual byte length.
        actual: u64,
        /// Maximum byte length.
        maximum: u64,
    },
    /// TOML or regex validation failed.
    #[error(transparent)]
    Invalid(#[from] DeclarativeError),
}

/// Load `.toml` adapters from one directory in deterministic file-name order.
pub fn load_adapters(path: &Path) -> Result<Vec<DeclarativeAdapter>, AdapterConfigError> {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(AdapterConfigError::Directory {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let mut files = entries
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|source| AdapterConfigError::Directory {
                    path: path.to_path_buf(),
                    source,
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    files.retain(|file| {
        file.extension()
            .is_some_and(|extension| extension == "toml")
    });
    files.sort();
    files.into_iter().map(|file| load_one(&file)).collect()
}

fn load_one(path: &Path) -> Result<DeclarativeAdapter, AdapterConfigError> {
    let metadata = std::fs::metadata(path).map_err(|source| AdapterConfigError::File {
        operation: "inspect",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.len() > MAX_ADAPTER_BYTES {
        return Err(AdapterConfigError::TooLarge {
            path: path.to_path_buf(),
            actual: metadata.len(),
            maximum: MAX_ADAPTER_BYTES,
        });
    }
    let text = std::fs::read_to_string(path).map_err(|source| AdapterConfigError::File {
        operation: "read",
        path: path.to_path_buf(),
        source,
    })?;
    DeclarativeAdapter::parse(path.display().to_string(), &text).map_err(Into::into)
}
