//! Bounded read-only tail for adopted process log files.
//!
//! The tail keeps one read-only handle open and compares it with the path on
//! every poll, so it handles append, partial lines, truncation, copy-truncate
//! and rename rotation, same-size replacement, and temporary absence without
//! ever writing, locking, or changing the file. A large existing file is not
//! replayed: reading starts at a bounded, line-aligned tail, and a resumable
//! [`LogCursor`] avoids duplicates after a daemon restart.

use std::{
    fs::{File, Metadata},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
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
    /// The path is no longer a safe log to follow.
    #[error("adopted log '{path}' is unsafe: {reason}")]
    Unsafe {
        /// Log path.
        path: PathBuf,
        /// Why it was refused.
        reason: String,
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

/// Bounds applied to one tail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogTailPolicy {
    /// Maximum bytes returned by one poll.
    pub read_bytes: usize,
    /// How much of an existing file is read when tailing starts without a cursor.
    pub initial_tail_bytes: u64,
    /// Unread bytes beyond which the tail skips ahead instead of falling behind.
    pub max_backlog_bytes: u64,
}

impl Default for LogTailPolicy {
    fn default() -> Self {
        LogTailPolicy {
            read_bytes: 64 * 1024,
            initial_tail_bytes: 64 * 1024,
            max_backlog_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Resumable position in one file identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogCursor {
    /// Filesystem device of the file.
    pub device: u64,
    /// Inode of the file.
    pub inode: u64,
    /// Next unread byte offset.
    pub offset: u64,
}

/// Something about the log that a caller may want to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogTailNotice {
    /// Tailing began this many bytes into an existing file.
    InitialTail {
        /// Bytes of history not read.
        skipped_bytes: u64,
    },
    /// Tailing continued from a persisted cursor.
    Resumed,
    /// A persisted cursor did not match the file, so the initial tail was used.
    CursorMismatch,
    /// The file shrank below the read offset (truncate or copy-truncate).
    Truncated,
    /// The path now names a different file (rotation or recreation).
    Replaced,
    /// The path disappeared; reading continues from the open handle.
    Missing,
    /// Unread bytes exceeded the backlog bound and were skipped.
    BacklogSkipped {
        /// Bytes not read.
        skipped_bytes: u64,
    },
}

/// One bounded poll result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LogTailChunk {
    /// Newly read bytes, at most the configured per-poll limit.
    pub bytes: Vec<u8>,
    /// Whether partial-line state must be discarded before these bytes.
    pub reset: bool,
    /// Reportable changes noticed during this poll.
    pub notices: Vec<LogTailNotice>,
}

/// Stateful read-only tail for one validated absolute log path.
pub struct LogTail {
    path: PathBuf,
    owner_uid: u32,
    policy: LogTailPolicy,
    file: File,
    identity: FileIdentity,
    offset: u64,
    missing: bool,
    pending: Vec<LogTailNotice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    device: u64,
    inode: u64,
}

impl LogTail {
    /// Open `path` read-only, resuming from `cursor` when it still matches.
    pub fn open(
        path: PathBuf,
        owner_uid: u32,
        policy: LogTailPolicy,
        cursor: Option<LogCursor>,
    ) -> Result<Self, LogTailError> {
        if policy.read_bytes == 0 {
            return Err(LogTailError::ZeroLimit { path });
        }
        let (mut file, metadata) = open_checked(&path, owner_uid)?;
        let identity = identity(&metadata);
        let length = metadata.len();
        let resumed = cursor.filter(|cursor| {
            cursor.device == identity.device
                && cursor.inode == identity.inode
                && cursor.offset <= length
        });
        let (offset, notice) = match (cursor, resumed) {
            (_, Some(cursor)) => (cursor.offset, Some(LogTailNotice::Resumed)),
            (Some(_), None) => (
                initial_offset(&mut file, &path, length, policy.initial_tail_bytes)?,
                Some(LogTailNotice::CursorMismatch),
            ),
            (None, None) => {
                let offset = initial_offset(&mut file, &path, length, policy.initial_tail_bytes)?;
                (
                    offset,
                    (offset > 0).then_some(LogTailNotice::InitialTail {
                        skipped_bytes: offset,
                    }),
                )
            }
        };
        Ok(LogTail {
            path,
            owner_uid,
            policy,
            file,
            identity,
            offset,
            missing: false,
            pending: notice.into_iter().collect(),
        })
    }

    /// Current resumable position.
    pub fn cursor(&self) -> LogCursor {
        LogCursor {
            device: self.identity.device,
            inode: self.identity.inode,
            offset: self.offset,
        }
    }

    /// Read the next bounded chunk, following truncation and rotation.
    ///
    /// The open handle is drained to its end before the path is re-checked,
    /// so bytes written to a just-rotated file are not lost.
    pub fn poll(&mut self) -> Result<LogTailChunk, LogTailError> {
        let mut chunk = LogTailChunk {
            notices: std::mem::take(&mut self.pending),
            ..LogTailChunk::default()
        };
        self.read_current(&mut chunk)?;
        if chunk.bytes.is_empty() {
            self.follow_path(&mut chunk)?;
        }
        Ok(chunk)
    }

    /// Read everything still available, bounded by the backlog limit.
    pub fn drain(&mut self) -> Result<LogTailChunk, LogTailError> {
        let mut drained = LogTailChunk::default();
        let limit = usize::try_from(self.policy.max_backlog_bytes).unwrap_or(usize::MAX);
        loop {
            let chunk = self.poll()?;
            drained.reset |= chunk.reset && drained.bytes.is_empty();
            drained.notices.extend(chunk.notices);
            if chunk.bytes.is_empty() || drained.bytes.len() >= limit {
                return Ok(drained);
            }
            drained.bytes.extend(chunk.bytes);
        }
    }

    fn read_current(&mut self, chunk: &mut LogTailChunk) -> Result<(), LogTailError> {
        let length = self
            .file
            .metadata()
            .map_err(|source| self.file_error("inspect", source))?
            .len();
        if length < self.offset {
            self.offset = 0;
            chunk.reset = true;
            chunk.notices.push(LogTailNotice::Truncated);
        }
        let unread = length - self.offset;
        if unread > self.policy.max_backlog_bytes {
            let start = initial_offset(
                &mut self.file,
                &self.path,
                length,
                self.policy.initial_tail_bytes,
            )?;
            chunk.reset = true;
            chunk.notices.push(LogTailNotice::BacklogSkipped {
                skipped_bytes: start - self.offset,
            });
            self.offset = start;
        }
        self.file
            .seek(SeekFrom::Start(self.offset))
            .map_err(|source| self.file_error("seek", source))?;
        let limit = u64::try_from(self.policy.read_bytes).unwrap_or(u64::MAX);
        let mut bytes = Vec::new();
        (&mut self.file)
            .take(limit)
            .read_to_end(&mut bytes)
            .map_err(|source| self.file_error("read", source))?;
        self.offset += u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        chunk.bytes = bytes;
        Ok(())
    }

    fn follow_path(&mut self, chunk: &mut LogTailChunk) -> Result<(), LogTailError> {
        let metadata = match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !self.missing {
                    self.missing = true;
                    chunk.notices.push(LogTailNotice::Missing);
                }
                return Ok(());
            }
            Err(source) => return Err(self.file_error("inspect", source)),
        };
        check_metadata(&self.path, &metadata, self.owner_uid)?;
        if identity(&metadata) == self.identity {
            self.missing = false;
            return Ok(());
        }
        let (file, metadata) = open_checked(&self.path, self.owner_uid)?;
        self.file = file;
        self.identity = identity(&metadata);
        self.offset = 0;
        self.missing = false;
        chunk.reset = true;
        chunk.notices.push(LogTailNotice::Replaced);
        self.read_current(chunk)
    }

    fn file_error(&self, operation: &'static str, source: std::io::Error) -> LogTailError {
        LogTailError::File {
            operation,
            path: self.path.clone(),
            source,
        }
    }
}

/// Open read-only, then prove the handle is the safe file the path names.
fn open_checked(path: &Path, owner_uid: u32) -> Result<(File, Metadata), LogTailError> {
    let file_error = |operation, source| LogTailError::File {
        operation,
        path: path.to_path_buf(),
        source,
    };
    let before = std::fs::symlink_metadata(path).map_err(|source| file_error("inspect", source))?;
    check_metadata(path, &before, owner_uid)?;
    let file = File::open(path).map_err(|source| file_error("open", source))?;
    let opened = file
        .metadata()
        .map_err(|source| file_error("inspect", source))?;
    if identity(&opened) != identity(&before) {
        return Err(LogTailError::Unsafe {
            path: path.to_path_buf(),
            reason: "path changed while it was being opened".into(),
        });
    }
    Ok((file, opened))
}

fn check_metadata(path: &Path, metadata: &Metadata, owner_uid: u32) -> Result<(), LogTailError> {
    let unsafe_path = |reason: String| LogTailError::Unsafe {
        path: path.to_path_buf(),
        reason,
    };
    if metadata.file_type().is_symlink() {
        return Err(unsafe_path("is a symlink".into()));
    }
    if !metadata.is_file() {
        return Err(unsafe_path("is not a regular file".into()));
    }
    if metadata.uid() != owner_uid {
        return Err(unsafe_path(format!(
            "owner uid {} does not match daemon uid {owner_uid}",
            metadata.uid()
        )));
    }
    Ok(())
}

/// Start of the bounded initial tail, moved past the first partial line.
fn initial_offset(
    file: &mut File,
    path: &Path,
    length: u64,
    tail_bytes: u64,
) -> Result<u64, LogTailError> {
    if length <= tail_bytes {
        return Ok(0);
    }
    let start = length - tail_bytes;
    let file_error = |operation, source| LogTailError::File {
        operation,
        path: path.to_path_buf(),
        source,
    };
    // Include the byte before `start`: if it ends a line, `start` is aligned.
    file.seek(SeekFrom::Start(start - 1))
        .map_err(|source| file_error("seek", source))?;
    let mut window = Vec::new();
    file.take(tail_bytes + 1)
        .read_to_end(&mut window)
        .map_err(|source| file_error("read", source))?;
    Ok(window
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(start, |newline| {
            start - 1 + u64::try_from(newline).unwrap_or(0) + 1
        }))
}

fn identity(metadata: &Metadata) -> FileIdentity {
    FileIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    }
}
