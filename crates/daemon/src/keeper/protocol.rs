//! The keeper protocol (`docs/R1_KEEPER_DESIGN.md` §5): one frame is a type
//! byte, a big-endian `u32` length, and a payload. Control frames carry
//! JSON; output, input and checkpoints carry raw bytes.

use hmac::{Hmac, Mac};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::Sha256;
use std::io::{self, Read, Write};

/// The protocol this build speaks.
pub const PROTOCOL: u32 = 1;
/// The oldest keeper protocol this build reclaims.
pub const PROTOCOL_MIN: u32 = 1;
/// Largest frame of any type.
pub const MAX_FRAME: usize = 64 * 1024;
/// Largest handshake frame.
pub const MAX_HANDSHAKE_FRAME: usize = 4 * 1024;
/// Largest checkpoint the keeper keeps (it arrives in many frames).
pub const MAX_CHECKPOINT: usize = 8 * 1024 * 1024;

/// What a frame is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// Daemon → keeper, the same in every version: hang up the child, exit.
    Shutdown = 0x00,
    /// Keeper → daemon, first frame of every connection.
    Hello = 0x01,
    /// Daemon → keeper: the proof and where to resume.
    Claim = 0x02,
    /// Keeper → daemon: where the output stands.
    Claimed = 0x03,
    /// Keeper → daemon: earlier output, in order.
    Replay = 0x04,
    /// Keeper → daemon: live output, in order.
    Output = 0x05,
    /// Daemon → keeper: bytes for the terminal.
    Input = 0x06,
    /// Daemon → keeper: `{"cols","rows"}`.
    Resize = 0x07,
    /// Daemon → keeper: `{"signal"}` for the child's process group.
    Signal = 0x08,
    /// Keeper → daemon: the child ended.
    Exited = 0x09,
    /// Either way: one piece of a screen checkpoint.
    Checkpoint = 0x0A,
    /// Keeper → daemon: output the daemon asked for is gone.
    Gap = 0x0B,
    /// Keeper → daemon: another daemon claimed this keeper.
    Superseded = 0x0C,
    /// Daemon → keeper: the exit status arrived.
    Ack = 0x0D,
    /// Keeper → daemon: every replayed byte has been sent; live output follows.
    ReplayDone = 0x0E,
}

impl Kind {
    fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0x00 => Kind::Shutdown,
            0x01 => Kind::Hello,
            0x02 => Kind::Claim,
            0x03 => Kind::Claimed,
            0x04 => Kind::Replay,
            0x05 => Kind::Output,
            0x06 => Kind::Input,
            0x07 => Kind::Resize,
            0x08 => Kind::Signal,
            0x09 => Kind::Exited,
            0x0A => Kind::Checkpoint,
            0x0B => Kind::Gap,
            0x0C => Kind::Superseded,
            0x0D => Kind::Ack,
            0x0E => Kind::ReplayDone,
            _ => return None,
        })
    }
}

/// One frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    /// Its type.
    pub kind: Kind,
    /// Its payload.
    pub payload: Vec<u8>,
}

impl Frame {
    /// A frame with a JSON payload.
    pub fn json(kind: Kind, value: &impl Serialize) -> io::Result<Self> {
        Ok(Frame {
            kind,
            payload: serde_json::to_vec(value)?,
        })
    }

    /// The payload as JSON.
    pub fn parse<T: DeserializeOwned>(&self) -> io::Result<T> {
        serde_json::from_slice(&self.payload)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }
}

/// Write one frame. Payloads over [`MAX_FRAME`] are refused.
pub fn write_frame(writer: &mut impl Write, kind: Kind, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame too large",
        ));
    }
    let length = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(kind as u8);
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(payload);
    writer.write_all(&frame)?;
    writer.flush()
}

/// Write `bytes` as as many frames of `kind` as it takes.
pub fn write_chunked(writer: &mut impl Write, kind: Kind, bytes: &[u8]) -> io::Result<()> {
    for chunk in bytes.chunks(MAX_FRAME) {
        write_frame(writer, kind, chunk)?;
    }
    Ok(())
}

/// Read one frame of at most `max` bytes. `Ok(None)` at a clean end of
/// stream; an unknown type or an oversize frame is an error.
pub fn read_frame(reader: &mut impl Read, max: usize) -> io::Result<Option<Frame>> {
    let mut header = [0_u8; 5];
    match reader.read(&mut header[..1])? {
        0 => return Ok(None),
        _ => reader.read_exact(&mut header[1..])?,
    }
    let kind = Kind::from_byte(header[0])
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown frame type"))?;
    let length = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if length > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    Ok(Some(Frame { kind, payload }))
}

/// `Hello`, keeper → daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// The keeper's protocol.
    pub protocol: u32,
    /// The build that started the keeper.
    pub keeper_version: String,
    /// The Session it keeps.
    pub session_id: u64,
    /// The keeper's PID.
    pub keeper_pid: u32,
    /// The shell's PID.
    pub shell_pid: u32,
    /// Fresh random hex the daemon proves the secret over.
    pub nonce: String,
}

/// `Claim`, daemon → keeper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    /// The daemon's protocol.
    pub protocol: u32,
    /// HMAC-SHA256(secret, nonce), hex.
    pub proof: String,
    /// Output offset the daemon has already logged and processed.
    pub resume_from: u64,
}

/// `Claimed`, keeper → daemon. Offsets count every byte read from the PTY.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claimed {
    /// Bytes read from the PTY so far.
    pub output_end: u64,
    /// The oldest byte still in the ring.
    pub ring_start: u64,
    /// Where the kept checkpoint stands, if there is one.
    pub checkpoint_at: Option<u64>,
    /// The PTY's size.
    pub size: (u16, u16),
    /// The child's exit code, if it has ended (`-1` for a signal).
    pub exited: Option<i32>,
}

/// `Gap`, keeper → daemon: bytes `from..to` are gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    /// First missing offset.
    pub from: u64,
    /// First offset still kept.
    pub to: u64,
}

/// `Resize`, daemon → keeper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resize {
    /// Columns.
    pub cols: u16,
    /// Rows.
    pub rows: u16,
}

/// The only signals a daemon may ask for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SignalName {
    /// Hang up (the daemon's Stop).
    Hup,
    /// Interrupt.
    Int,
    /// Terminate.
    Term,
    /// Kill.
    Kill,
}

/// `Signal`, daemon → keeper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    /// Which.
    pub signal: SignalName,
}

/// `Exited`, keeper → daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exited {
    /// The exit code; `-1` when a signal ended it.
    pub code: i32,
}

/// Checkpoint pieces carry an 8-byte offset and a 1-byte "last" flag before
/// their data.
pub fn checkpoint_piece(at: u64, last: bool, data: &[u8]) -> Vec<u8> {
    let mut piece = Vec::with_capacity(9 + data.len());
    piece.extend_from_slice(&at.to_be_bytes());
    piece.push(u8::from(last));
    piece.extend_from_slice(data);
    piece
}

/// `(at, last, data)` of one checkpoint piece.
pub fn parse_checkpoint_piece(payload: &[u8]) -> io::Result<(u64, bool, &[u8])> {
    if payload.len() < 9 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "short checkpoint piece",
        ));
    }
    let mut at = [0_u8; 8];
    at.copy_from_slice(&payload[..8]);
    Ok((u64::from_be_bytes(at), payload[8] == 1, &payload[9..]))
}

/// Write a whole checkpoint as pieces under [`MAX_FRAME`].
pub fn write_checkpoint(writer: &mut impl Write, at: u64, snapshot: &[u8]) -> io::Result<()> {
    let room = MAX_FRAME - 9;
    let pieces: Vec<&[u8]> = if snapshot.is_empty() {
        vec![&[]]
    } else {
        snapshot.chunks(room).collect()
    };
    let count = pieces.len();
    for (index, data) in pieces.into_iter().enumerate() {
        write_frame(
            writer,
            Kind::Checkpoint,
            &checkpoint_piece(at, index + 1 == count, data),
        )?;
    }
    Ok(())
}

/// Hex of `bytes`.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Bytes of `hex`, if it is hex.
pub fn unhex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok())
        .collect()
}

/// The daemon's proof that it holds `secret`: HMAC-SHA256 over the nonce.
pub fn proof(secret: &[u8], nonce: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).unwrap_or_else(|_| {
        // HMAC takes a key of any length; this cannot fail.
        unreachable!("HMAC key")
    });
    mac.update(nonce.as_bytes());
    hex(&mac.finalize().into_bytes())
}

/// Check a proof in constant time.
pub fn proof_matches(secret: &[u8], nonce: &str, offered: &str) -> bool {
    let Some(offered) = unhex(offered) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret) else {
        return false;
    };
    mac.update(nonce.as_bytes());
    mac.verify_slice(&offered).is_ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_and_a_clean_end_is_none() {
        let mut wire = Vec::new();
        write_frame(&mut wire, Kind::Output, b"hello").unwrap();
        write_frame(&mut wire, Kind::Ack, b"{}").unwrap();
        let mut reader = &wire[..];
        assert_eq!(
            read_frame(&mut reader, MAX_FRAME).unwrap().unwrap(),
            Frame {
                kind: Kind::Output,
                payload: b"hello".to_vec()
            }
        );
        assert_eq!(
            read_frame(&mut reader, MAX_FRAME).unwrap().unwrap().kind,
            Kind::Ack
        );
        assert_eq!(read_frame(&mut reader, MAX_FRAME).unwrap(), None);
    }

    #[test]
    fn oversize_unknown_and_cut_frames_are_refused() {
        assert!(write_frame(&mut Vec::new(), Kind::Output, &vec![0; MAX_FRAME + 1]).is_err());
        let mut big = vec![Kind::Hello as u8];
        big.extend_from_slice(&5_000_u32.to_be_bytes());
        big.extend(vec![b'x'; 5_000]);
        assert!(read_frame(&mut &big[..], MAX_HANDSHAKE_FRAME).is_err());
        assert!(read_frame(&mut &[0x7f_u8, 0, 0, 0, 0][..], MAX_FRAME).is_err());
        assert!(read_frame(&mut &[Kind::Output as u8, 0, 0, 0, 9, b'a'][..], MAX_FRAME).is_err());
    }

    #[test]
    fn long_output_goes_in_frames_under_the_limit() {
        let mut wire = Vec::new();
        let bytes = vec![b'y'; MAX_FRAME * 2 + 10];
        write_chunked(&mut wire, Kind::Replay, &bytes).unwrap();
        let mut reader = &wire[..];
        let mut back = Vec::new();
        while let Some(frame) = read_frame(&mut reader, MAX_FRAME).unwrap() {
            back.extend(frame.payload);
        }
        assert_eq!(back, bytes);
    }

    #[test]
    fn a_checkpoint_comes_back_whole_with_its_offset() {
        let snapshot: Vec<u8> = (0..200_000_u32).map(|n| (n % 251) as u8).collect();
        let mut wire = Vec::new();
        write_checkpoint(&mut wire, 1_290_000, &snapshot).unwrap();
        let mut reader = &wire[..];
        let mut back = Vec::new();
        let mut finished = false;
        while let Some(frame) = read_frame(&mut reader, MAX_FRAME).unwrap() {
            let (at, last, data) = parse_checkpoint_piece(&frame.payload).unwrap();
            assert_eq!(at, 1_290_000);
            back.extend_from_slice(data);
            finished = last;
        }
        assert!(finished);
        assert_eq!(back, snapshot);
    }

    #[test]
    fn the_proof_needs_the_secret_and_the_nonce() {
        let secret = [7_u8; 32];
        let offered = proof(&secret, "9f1c");
        assert_eq!(offered.len(), 64);
        assert!(proof_matches(&secret, "9f1c", &offered));
        assert!(!proof_matches(&secret, "9f1d", &offered));
        assert!(!proof_matches(&[8_u8; 32], "9f1c", &offered));
        assert!(!proof_matches(&secret, "9f1c", "not hex"));
        assert_eq!(unhex(&hex(&secret)).unwrap(), secret);
    }

    #[test]
    fn handshake_json_is_what_the_design_shows() {
        let hello = Hello {
            protocol: 1,
            keeper_version: "0.3.0".into(),
            session_id: 42,
            keeper_pid: 51_234,
            shell_pid: 51_240,
            nonce: "9f1c".into(),
        };
        let frame = Frame::json(Kind::Hello, &hello).unwrap();
        assert_eq!(frame.parse::<Hello>().unwrap(), hello);
        let signal: Signal = serde_json::from_str(r#"{"signal":"HUP"}"#).unwrap();
        assert_eq!(signal.signal, SignalName::Hup);
    }
}
