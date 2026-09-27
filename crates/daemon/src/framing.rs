//! Binary WebSocket framing for terminal bytes.

use protocol::SessionId;

const HEADER_LEN: usize = 8;

/// Errors produced while decoding terminal binary frames.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// A frame did not contain the eight-byte session header.
    #[error("terminal frame needs an 8-byte session header, got {byte_count} bytes")]
    MissingHeader {
        /// Number of bytes received.
        byte_count: usize,
    },
}

/// Prefix terminal bytes with a big-endian session ID.
pub fn encode_terminal_frame(session_id: SessionId, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(HEADER_LEN + payload.len());
    frame.extend_from_slice(&session_id.0.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Split a terminal binary frame into its session ID and payload.
pub fn decode_terminal_frame(frame: &[u8]) -> Result<(SessionId, &[u8]), FrameError> {
    let header: [u8; HEADER_LEN] = frame
        .get(..HEADER_LEN)
        .ok_or(FrameError::MissingHeader {
            byte_count: frame.len(),
        })?
        .try_into()
        .map_err(|_| FrameError::MissingHeader {
            byte_count: frame.len(),
        })?;
    Ok((SessionId(u64::from_be_bytes(header)), &frame[HEADER_LEN..]))
}
