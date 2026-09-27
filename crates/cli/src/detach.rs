//! Recognition of the `Ctrl-\ d` local detach sequence.

const CONTROL_BACKSLASH: u8 = 0x1c;

/// Bytes to forward and whether the local client should detach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachResult {
    /// Input bytes that are not part of the detach sequence.
    pub bytes: Vec<u8>,
    /// True once `Ctrl-\ d` has been consumed.
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
