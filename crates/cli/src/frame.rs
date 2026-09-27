//! Binary WebSocket framing for terminal bytes.

use protocol::SessionId;

const HEADER_LEN: usize = 8;

/// Error returned for a binary frame without a complete session header.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("terminal frame needs an 8-byte session header, got {byte_count} bytes")]
pub struct FrameError {
    /// Number of bytes received.
    pub byte_count: usize,
}

/// Prefix terminal bytes with a big-endian session ID.
pub fn encode_terminal_frame(session_id: SessionId, payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(HEADER_LEN + payload.len());
    frame.extend_from_slice(&session_id.0.to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

/// Split a terminal frame into its session ID and payload.
pub fn decode_terminal_frame(frame: &[u8]) -> Result<(SessionId, &[u8]), FrameError> {
    let header = frame.get(..HEADER_LEN).ok_or(FrameError {
        byte_count: frame.len(),
    })?;
    let mut session = [0_u8; HEADER_LEN];
    session.copy_from_slice(header);
    Ok((SessionId(u64::from_be_bytes(session)), &frame[HEADER_LEN..]))
}
