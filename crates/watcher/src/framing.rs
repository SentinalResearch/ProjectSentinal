//! Streaming ANSI removal and bounded line framing.

use crate::observation::{truncate_utf8, MAX_OUTPUT_LINE_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscapeState {
    Ground,
    Escape,
    Csi,
    Osc,
    OscEscape,
}

/// Streaming ANSI-free line splitter for terminal and tailed-log bytes.
pub struct LineFramer {
    maximum: usize,
    line: Vec<u8>,
    state: EscapeState,
    /// The unfinished line last returned by [`Self::new_pending_prompt`].
    offered: Option<String>,
}

impl Default for LineFramer {
    fn default() -> Self {
        Self::new(MAX_OUTPUT_LINE_BYTES)
    }
}

impl LineFramer {
    /// Create a line framer with a byte bound per line.
    pub fn new(maximum: usize) -> Self {
        LineFramer {
            maximum,
            line: Vec::new(),
            state: EscapeState::Ground,
            offered: None,
        }
    }

    /// Consume bytes and return complete ANSI-free lines.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for byte in bytes {
            match self.state {
                EscapeState::Ground => match *byte {
                    0x1b => self.state = EscapeState::Escape,
                    b'\n' | b'\r' => self.emit(&mut lines),
                    value
                        if (value >= b' ' || value == b'\t') && self.line.len() < self.maximum =>
                    {
                        self.line.push(value);
                    }
                    _ => {}
                },
                EscapeState::Escape => {
                    self.state = match *byte {
                        b'[' => EscapeState::Csi,
                        b']' => EscapeState::Osc,
                        _ => EscapeState::Ground,
                    };
                }
                EscapeState::Csi => {
                    if (0x40..=0x7e).contains(byte) {
                        self.state = EscapeState::Ground;
                    }
                }
                EscapeState::Osc => match *byte {
                    0x07 => self.state = EscapeState::Ground,
                    0x1b => self.state = EscapeState::OscEscape,
                    _ => {}
                },
                EscapeState::OscEscape => {
                    self.state = if *byte == b'\\' {
                        EscapeState::Ground
                    } else {
                        EscapeState::Osc
                    };
                }
            }
        }
        lines
    }

    /// Return the current incomplete ANSI-free line without consuming it.
    pub fn pending(&self) -> String {
        truncate_utf8(&String::from_utf8_lossy(&self.line), self.maximum)
    }

    /// The unfinished line, the first time it reads as a prompt. A prompt leaves
    /// the cursor on its line, so without this no newline would ever report it.
    pub fn new_pending_prompt(&mut self) -> Option<String> {
        let pending = self.pending();
        if crate::generic::prompt_line(&pending).is_none()
            || self.offered.as_ref() == Some(&pending)
        {
            return None;
        }
        self.offered = Some(pending.clone());
        Some(pending)
    }

    /// Consume the current incomplete line at end-of-stream.
    pub fn finish(&mut self) -> Option<String> {
        if self.line.is_empty() {
            return None;
        }
        let value = self.pending();
        self.line.clear();
        self.offered = None;
        Some(value)
    }

    /// Discard an incomplete line and ANSI state after log rotation or truncation.
    pub fn reset(&mut self) {
        self.line.clear();
        self.state = EscapeState::Ground;
        self.offered = None;
    }

    fn emit(&mut self, lines: &mut Vec<String>) {
        if self.line.is_empty() {
            return;
        }
        lines.push(self.pending());
        self.line.clear();
        self.offered = None;
    }
}
