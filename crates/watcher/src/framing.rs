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

    /// Consume the current incomplete line at end-of-stream.
    pub fn finish(&mut self) -> Option<String> {
        if self.line.is_empty() {
            return None;
        }
        let value = self.pending();
        self.line.clear();
        Some(value)
    }

    /// Discard an incomplete line and ANSI state after log rotation or truncation.
    pub fn reset(&mut self) {
        self.line.clear();
        self.state = EscapeState::Ground;
    }

    fn emit(&mut self, lines: &mut Vec<String>) {
        if self.line.is_empty() {
            return;
        }
        lines.push(self.pending());
        self.line.clear();
    }
}
