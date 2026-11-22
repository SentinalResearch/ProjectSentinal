//! Terminal output cut only between whole sequences (T1.5).
//!
//! A PTY read can end inside an escape sequence or a UTF-8 character. A
//! viewer whose snapshot was taken there would never see the start, and
//! would draw the rest (`1m`, a lone continuation byte) as text. The reader
//! therefore passes the screen parser and every viewer only whole sequences,
//! holding an unfinished tail back until the next read completes it; a
//! snapshot then always falls between sequences. Until the authoritative
//! screen of T2.
//!
//! The scanner knows the shapes, not the meanings: ESC sequences, CSI, OSC
//! (ended by BEL or ST), DCS/SOS/PM/APC strings (ended by ST), and UTF-8.

/// Most bytes held back. A sequence longer than this (a huge OSC title) is
/// released as it is rather than withheld.
pub(crate) const MAX_HELD_BYTES: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    Esc,
    EscIntermediate,
    Csi,
    /// OSC: ends at BEL or ST.
    Osc,
    /// DCS, SOS, PM or APC: ends at ST.
    Text,
    /// ESC inside a string: `\` ends it (ST).
    TextEsc {
        osc: bool,
    },
    Utf8 {
        remaining: u8,
    },
}

/// Holds back the unfinished tail of terminal output between reads.
#[derive(Debug, Default)]
pub(crate) struct SequenceBoundary {
    state: State,
    held: Vec<u8>,
}

impl SequenceBoundary {
    /// `bytes` read now, as whole sequences: whatever was held, then this
    /// read up to its last complete sequence. The rest is held.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut last_complete = None;
        for (index, byte) in bytes.iter().enumerate() {
            self.state = step(self.state, *byte);
            if self.state == State::Ground {
                last_complete = Some(index);
            }
        }
        let mut out = std::mem::take(&mut self.held);
        match last_complete {
            Some(end) => {
                out.extend_from_slice(&bytes[..=end]);
                self.held.extend_from_slice(&bytes[end + 1..]);
            }
            None => self.held = [out.as_slice(), bytes].concat(),
        }
        if last_complete.is_none() {
            out = Vec::new();
        }
        if self.held.len() > MAX_HELD_BYTES {
            out.append(&mut self.held);
            self.state = State::Ground;
        }
        out
    }

    /// The output ended: release whatever is held.
    pub(crate) fn finish(&mut self) -> Vec<u8> {
        self.state = State::Ground;
        std::mem::take(&mut self.held)
    }
}

fn step(state: State, byte: u8) -> State {
    match state {
        State::Ground | State::Utf8 { .. } if byte == 0x1b => State::Esc,
        State::Ground => match byte {
            0xc0..=0xdf => State::Utf8 { remaining: 1 },
            0xe0..=0xef => State::Utf8 { remaining: 2 },
            0xf0..=0xf7 => State::Utf8 { remaining: 3 },
            _ => State::Ground,
        },
        State::Utf8 { remaining } => match byte {
            0x80..=0xbf if remaining > 1 => State::Utf8 {
                remaining: remaining - 1,
            },
            // The last continuation byte, or an invalid one: the character is over.
            _ => State::Ground,
        },
        State::Esc => match byte {
            b'[' => State::Csi,
            b']' => State::Osc,
            b'P' | b'X' | b'^' | b'_' => State::Text,
            0x20..=0x2f => State::EscIntermediate,
            0x1b => State::Esc,
            _ => State::Ground,
        },
        State::EscIntermediate => match byte {
            0x20..=0x2f => State::EscIntermediate,
            0x1b => State::Esc,
            _ => State::Ground,
        },
        State::Csi => match byte {
            0x40..=0x7e => State::Ground,
            0x1b => State::Esc,
            // CAN and SUB cancel a sequence.
            0x18 | 0x1a => State::Ground,
            _ => State::Csi,
        },
        State::Osc => match byte {
            0x07 => State::Ground,
            0x1b => State::TextEsc { osc: true },
            0x18 | 0x1a => State::Ground,
            _ => State::Osc,
        },
        State::Text => match byte {
            0x1b => State::TextEsc { osc: false },
            0x18 | 0x1a => State::Ground,
            _ => State::Text,
        },
        State::TextEsc { osc } => match byte {
            b'\\' => State::Ground,
            0x1b => State::TextEsc { osc },
            _ if osc => State::Osc,
            _ => State::Text,
        },
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A recorded-like stream: colours, cursor moves, an OSC title (BEL and
    /// ST), the alternate screen, a DCS string, and multi-byte UTF-8.
    fn stream() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"\x1b]0;build \xe2\x9c\x85 title\x07");
        bytes.extend_from_slice(b"\x1b[1;32mOK\x1b[0m compiling \xe6\xbc\xa2\xe5\xad\x97\r\n");
        bytes.extend_from_slice(
            b"\x1b[?1049h\x1b[2J\x1b[H\x1b[38;5;208m\xf0\x9f\xa7\xaa test\x1b[0m",
        );
        bytes.extend_from_slice(b"\x1b]8;;https://example.invalid\x1b\\link\x1b]8;;\x1b\\");
        bytes.extend_from_slice(b"\x1bP+q544e\x1b\\\x1b(B\x1b[5;10Hdone \xc3\xa9t\xc3\xa9\r\n");
        // Not `\x1b[?1049l`: a snapshot carries only the screen on show, so
        // leaving the alternate screen after an attach is T2's to fix.
        bytes.extend_from_slice(b"\x1b[3mitalic\x1b[23m end");
        bytes
    }

    fn feed(parser: &mut vt100::Parser, boundary: &mut SequenceBoundary, bytes: &[u8]) {
        parser.process(&boundary.push(bytes));
    }

    #[test]
    fn a_viewer_attaching_at_any_byte_ends_with_the_daemon_s_screen() {
        let stream = stream();
        for cut in 0..=stream.len() {
            let mut daemon = vt100::Parser::new(12, 60, 0);
            let mut boundary = SequenceBoundary::default();
            feed(&mut daemon, &mut boundary, &stream[..cut]);
            // A viewer attaches here: the snapshot, then everything after it.
            let snapshot = daemon.screen().state_formatted();
            let mut viewer = vt100::Parser::new(12, 60, 0);
            viewer.process(&snapshot);
            let rest = boundary.push(&stream[cut..]);
            daemon.process(&rest);
            viewer.process(&rest);
            let tail = boundary.finish();
            daemon.process(&tail);
            viewer.process(&tail);
            assert_eq!(
                viewer.screen().contents_formatted(),
                daemon.screen().contents_formatted(),
                "viewer attached at byte {cut} drew a different screen"
            );
        }
    }

    #[test]
    fn without_holding_back_a_cut_sequence_shows_as_text() {
        // The bug this fixes: "\x1b[3" | "1m" draws "1m".
        let mut daemon = vt100::Parser::new(2, 20, 0);
        daemon.process(b"a\x1b[3");
        let mut viewer = vt100::Parser::new(2, 20, 0);
        viewer.process(&daemon.screen().state_formatted());
        viewer.process(b"1mb");
        assert!(viewer.screen().contents().contains("1m"));
    }

    #[test]
    fn a_split_utf8_character_is_held_until_it_is_whole() {
        let mut boundary = SequenceBoundary::default();
        let check = "✅".as_bytes();
        assert_eq!(boundary.push(&check[..1]), b"");
        assert_eq!(boundary.push(&check[1..]), check);
        assert_eq!(boundary.push(b"plain"), b"plain");
    }

    #[test]
    fn a_runaway_sequence_is_released_not_withheld_forever() {
        let mut boundary = SequenceBoundary::default();
        let mut title = b"\x1b]0;".to_vec();
        title.extend(std::iter::repeat_n(b'x', MAX_HELD_BYTES + 10));
        assert_eq!(boundary.push(&title).len(), title.len());
        assert_eq!(boundary.push(b"ok"), b"ok");
    }
}
