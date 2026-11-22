//! Recognition of the `Ctrl+J` and legacy `Ctrl-\ d` local detach shortcuts.
//!
//! `Ctrl+J` is the line-feed byte, which also arrives inside pasted text and
//! input other programs write. It detaches only as a keypress: the last byte of
//! a read, outside a bracketed paste. Any other line feed is forwarded.

const CONTROL_BACKSLASH: u8 = 0x1c;
const CONTROL_J: u8 = 0x0a;
const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";

/// Bytes to forward and whether the local client should detach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachResult {
    /// Input bytes that are not part of the detach sequence.
    pub bytes: Vec<u8>,
    /// True once a local detach shortcut has been consumed.
    pub detach: bool,
}

/// Stateful detector supporting a detach sequence split across reads.
#[derive(Debug, Default)]
pub struct DetachDetector {
    pending_control: bool,
    in_paste: bool,
}

impl DetachDetector {
    /// Create a detector with no pending bytes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume a chunk and remove the first complete detach sequence.
    pub fn push(&mut self, input: &[u8]) -> DetachResult {
        let mut bytes = Vec::with_capacity(input.len());
        for (index, byte) in input.iter().enumerate() {
            let rest = &input[index..];
            if rest.starts_with(PASTE_START) {
                self.in_paste = true;
            } else if rest.starts_with(PASTE_END) {
                self.in_paste = false;
            }
            let keypress = index + 1 == input.len() && !self.in_paste;
            if *byte == CONTROL_J && keypress {
                self.pending_control = false;
                return DetachResult {
                    bytes,
                    detach: true,
                };
            }
            if self.pending_control {
                self.pending_control = false;
                if *byte == b'd' {
                    return DetachResult {
                        bytes,
                        detach: true,
                    };
                }
                bytes.push(CONTROL_BACKSLASH);
            }
            if *byte == CONTROL_BACKSLASH {
                self.pending_control = true;
            } else {
                bytes.push(*byte);
            }
        }
        DetachResult {
            bytes,
            detach: false,
        }
    }

    /// Flush a trailing `Ctrl-\` when input reaches EOF.
    pub fn finish(&mut self) -> Vec<u8> {
        if self.pending_control {
            self.pending_control = false;
            vec![CONTROL_BACKSLASH]
        } else {
            vec![]
        }
    }
}

/// `sentinal detach`: ask the daemon to make this Session's attached
/// terminals let go. Only works from inside the Session, which the daemon
/// checks with the OS.
pub async fn detach_command(err: &mut dyn std::io::Write) -> std::process::ExitCode {
    use crate::hook_report::{exchange, socket_path};
    use protocol::{LocalRequest, LocalResponse, SessionId};
    use std::{path::PathBuf, process::ExitCode, time::Duration};

    let Some(id) = std::env::var("SENTINAL_SESSION_ID")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|id| *id > 0)
    else {
        let _ = writeln!(
            err,
            "sentinal: not inside a Sentinal Session; nothing to detach"
        );
        return ExitCode::FAILURE;
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let explicit = std::env::var_os(protocol::local::SOCKET_ENV).map(PathBuf::from);
    let Some(socket) = socket_path(explicit, home.as_deref(), runtime.as_deref()) else {
        let _ = writeln!(err, "sentinal: HOME is not set; set SENTINAL_SOCKET");
        return ExitCode::FAILURE;
    };
    let request = LocalRequest::Detach {
        session_id: SessionId(id),
        caller_pid: None,
    };
    match tokio::time::timeout(Duration::from_secs(2), exchange(&socket, request)).await {
        Ok(Ok(LocalResponse::Session { .. })) => ExitCode::SUCCESS,
        Ok(Ok(LocalResponse::Error { message, .. })) => {
            let _ = writeln!(err, "sentinal: detach refused: {message}");
            ExitCode::FAILURE
        }
        _ => {
            let _ = writeln!(
                err,
                "sentinal: sentinald did not answer at {}",
                socket.display()
            );
            ExitCode::FAILURE
        }
    }
}
