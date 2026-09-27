//! Terminal frame behaviour tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::framing::{decode_terminal_frame, encode_terminal_frame, FrameError};
use protocol::SessionId;

#[test]
fn terminal_frame_round_trips() {
    let encoded = encode_terminal_frame(SessionId(0x0102_0304_0506_0708), b"hello");

    let (session_id, payload) = decode_terminal_frame(&encoded).unwrap();

    assert_eq!(session_id, SessionId(0x0102_0304_0506_0708));
    assert_eq!(payload, b"hello");
    assert_eq!(&encoded[..8], &[1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn terminal_frame_rejects_a_missing_header() {
    let error = decode_terminal_frame(&[0; 7]).unwrap_err();

    assert_eq!(error, FrameError::MissingHeader { byte_count: 7 });
}
