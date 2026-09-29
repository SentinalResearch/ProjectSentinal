//! Bounded replay of persisted FULL Session output.

use super::{SessionManager, SessionManagerError};
use protocol::{CaptureLevel, SessionId};
use std::{fs::File, io::Read};

/// Default decompressed bytes returned by one replay request.
pub const DEFAULT_OUTPUT_PAGE_BYTES: usize = 256 * 1024;
/// Maximum decompressed bytes returned by one replay request.
pub const MAX_OUTPUT_PAGE_BYTES: usize = 1024 * 1024;
/// Maximum decompressed offset plus page length scanned by one request.
pub const MAX_OUTPUT_SCAN_BYTES: u64 = 64 * 1024 * 1024;

/// One page of persisted terminal bytes and its continuation metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputReplay {
    /// Decompressed ANSI terminal bytes.
    pub bytes: Vec<u8>,
    /// Requested decompressed offset.
    pub offset: u64,
    /// Offset to use for the next page.
    pub next_offset: u64,
    /// Whether the decoder reached the end of available output.
    pub complete: bool,
    /// Stable warning code when the final zstd frame was corrupt or truncated.
    pub warning: Option<&'static str>,
}

impl SessionManager {
    /// Read one bounded page from a Session's compressed terminal log.
    pub fn replay_output(
        &self,
        session_id: SessionId,
        offset: u64,
        limit: usize,
    ) -> Result<OutputReplay, SessionManagerError> {
        validate_range(offset, limit)?;
        let session = self.known_session(session_id)?;
        if session.capture != CaptureLevel::Full {
            return Err(SessionManagerError::OutputUnavailable {
                session_id,
                capture: session.capture,
            });
        }
        let path = self
            .inner
            .config
            .data_dir
            .join("sessions")
            .join(format!("{}.log.zst", session_id.0));
        let file = File::open(&path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                SessionManagerError::OutputNotFound(session_id)
            } else {
                SessionManagerError::OutputRead { session_id, source }
            }
        })?;
        Ok(read_page(file, offset, limit))
    }
}

fn validate_range(offset: u64, limit: usize) -> Result<(), SessionManagerError> {
    let limit_u64 = u64::try_from(limit).unwrap_or(u64::MAX);
    let end = offset.checked_add(limit_u64);
    if limit == 0
        || limit > MAX_OUTPUT_PAGE_BYTES
        || end.is_none_or(|end| end > MAX_OUTPUT_SCAN_BYTES)
    {
        return Err(SessionManagerError::InvalidOutputRange {
            offset,
            limit,
            max_page: MAX_OUTPUT_PAGE_BYTES,
            max_scan: MAX_OUTPUT_SCAN_BYTES,
        });
    }
    Ok(())
}

fn read_page(file: File, offset: u64, limit: usize) -> OutputReplay {
    let mut decoder = match zstd::stream::read::Decoder::new(file) {
        Ok(decoder) => decoder,
        Err(_) => {
            return OutputReplay {
                bytes: Vec::new(),
                offset,
                next_offset: offset,
                complete: true,
                warning: Some("TRUNCATED_ZSTD"),
            };
        }
    };
    let end = offset.saturating_add(u64::try_from(limit).unwrap_or(u64::MAX));
    let mut position = 0_u64;
    let mut bytes = Vec::with_capacity(limit);
    let mut buffer = [0_u8; 8192];
    let mut complete = false;
    let mut warning = None;

    loop {
        let read_limit = if position >= end {
            1
        } else {
            usize::try_from((end - position).min(buffer.len() as u64)).unwrap_or(buffer.len())
        };
        match decoder.read(&mut buffer[..read_limit]) {
            Ok(0) => {
                complete = true;
                break;
            }
            Ok(count) => {
                let count_u64 = u64::try_from(count).unwrap_or(u64::MAX);
                let chunk_end = position.saturating_add(count_u64);
                let copy_start = offset.saturating_sub(position).min(count_u64);
                let copy_end = end.saturating_sub(position).min(count_u64);
                if copy_start < copy_end {
                    let start = usize::try_from(copy_start).unwrap_or(count);
                    let finish = usize::try_from(copy_end).unwrap_or(count);
                    bytes.extend_from_slice(&buffer[start..finish]);
                }
                position = chunk_end;
                if position > end {
                    break;
                }
            }
            Err(_) => {
                complete = true;
                warning = Some("TRUNCATED_ZSTD");
                break;
            }
        }
    }

    OutputReplay {
        next_offset: offset.saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX)),
        bytes,
        offset,
        complete,
        warning,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_and_excessive_ranges_are_rejected() {
        assert!(validate_range(0, 0).is_err());
        assert!(validate_range(0, MAX_OUTPUT_PAGE_BYTES + 1).is_err());
        assert!(validate_range(MAX_OUTPUT_SCAN_BYTES, 1).is_err());
    }
}
