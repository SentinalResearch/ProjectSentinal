//! Bounded read-only polling for adopted process log files.

use std::{
    fs::{File, Metadata},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::PathBuf,
};

/// Failure opening or reading an adopted log.
#[derive(Debug, thiserror::Error)]
pub enum LogTailError {
    /// The requested per-poll limit was zero.
    #[error("invalid adopted log read limit 0 for '{path}'")]
    ZeroLimit {
        /// Log path.
        path: PathBuf,
    },
    /// The log could not be inspected, opened, sought, or read.
    #[error("failed to {operation} adopted log '{path}': {source}")]
    File {
        /// Operation that failed.
        operation: &'static str,
        /// Log path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
}

/// One bounded poll result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogTailChunk {
    /// Newly appended bytes, at most the configured per-poll limit.
    pub bytes: Vec<u8>,
    /// Whether truncation or rotation invalidated partial-line state.
    pub reset: bool,
}

/// Stateful read-only tail for one validated absolute log path.
pub struct LogTail {
    path: PathBuf,
    maximum: usize,
    offset: u64,
    identity: FileIdentity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl LogTail {
    /// Open a log tail that begins at byte zero and bounds each poll.
    pub fn open(path: PathBuf, maximum: usize) -> Result<Self, LogTailError> {
        if maximum == 0 {
            return Err(LogTailError::ZeroLimit { path });
        }
        let metadata = metadata(&path)?;
        Ok(LogTail {
            path,
            maximum,
            offset: 0,
            identity: identity(&metadata),
        })
    }

    /// Read the next bounded append, resetting after truncation or rotation.
    pub fn poll(&mut self) -> Result<LogTailChunk, LogTailError> {
        let metadata = metadata(&self.path)?;
        let current = identity(&metadata);
        let reset = current != self.identity || metadata.len() < self.offset;
        if reset {
            self.offset = 0;
            self.identity = current;
        }
        let mut file = File::open(&self.path).map_err(|source| LogTailError::File {
            operation: "open",
            path: self.path.clone(),
            source,
        })?;
        file.seek(SeekFrom::Start(self.offset))
            .map_err(|source| LogTailError::File {
                operation: "seek",
                path: self.path.clone(),
                source,
            })?;
        let limit = u64::try_from(self.maximum).unwrap_or(u64::MAX);
        let mut bytes = Vec::with_capacity(self.maximum.min(8 * 1024));
        file.take(limit)
            .read_to_end(&mut bytes)
            .map_err(|source| LogTailError::File {
                operation: "read",
                path: self.path.clone(),
                source,
            })?;
        self.offset = self
            .offset
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        Ok(LogTailChunk { bytes, reset })
    }
}

fn metadata(path: &PathBuf) -> Result<Metadata, LogTailError> {
    std::fs::metadata(path).map_err(|source| LogTailError::File {
        operation: "inspect",
        path: path.clone(),
        source,
    })
}

fn identity(metadata: &Metadata) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}
