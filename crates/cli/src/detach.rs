//! Recognition of the `Ctrl+J` and legacy `Ctrl-\ d` local detach shortcuts.

const CONTROL_BACKSLASH: u8 = 0x1c;
const CONTROL_J: u8 = 0x0a;

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
}

impl DetachDetector {
    /// Create a detector with no pending bytes.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume a chunk and remove the first complete detach sequence.
    pub fn push(&mut self, input: &[u8]) -> DetachResult {
        let mut bytes = Vec::with_capacity(input.len());
        for byte in input {
            if *byte == CONTROL_J {
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
