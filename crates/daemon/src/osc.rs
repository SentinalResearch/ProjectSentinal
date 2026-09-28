//! Streaming shell-integration OSC parsing without altering terminal bytes.

const OSC_PREFIX: &[u8] = b"\x1b]";
const BEL: u8 = 0x07;
const MAX_OSC_BYTES: usize = 4 * 1024;

/// Structured shell lifecycle information carried in OSC sequences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellSignal {
    /// The shell is about to draw a prompt.
    PromptStart,
    /// The shell accepted a command.
    CommandStart,
    /// Command output is about to begin.
    OutputStart,
    /// The command completed with this exit code.
    CommandEnd(i32),
    /// The shell changed its working directory.
    Cwd(String),
    /// Exact command text reported by the shell hook.
    CommandText(String),
}

/// Output and signals produced by one parser call.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParsedOsc {
    /// Original PTY bytes, byte-for-byte, once complete enough to classify.
    pub output: Vec<u8>,
    /// Recognised shell signals in stream order.
    pub signals: Vec<ShellSignal>,
}

/// Incremental parser for OSC 133, OSC 7, and terminal-agent OSC 7420.
#[derive(Debug, Default)]
pub struct OscParser {
    pending: Vec<u8>,
}

impl OscParser {
    /// Create an empty parser.
    pub fn new() -> Self {
        Self::default()
    }

    /// Consume PTY bytes and return pass-through bytes plus complete signals.
    pub fn push(&mut self, bytes: &[u8]) -> ParsedOsc {
        self.pending.extend_from_slice(bytes);
        let mut parsed = ParsedOsc::default();

        loop {
            let Some(start) = find_subslice(&self.pending, OSC_PREFIX) else {
                let keep = usize::from(self.pending.last() == Some(&0x1b));
                let emit = self.pending.len().saturating_sub(keep);
                parsed.output.extend(self.pending.drain(..emit));
                break;
            };
            if start > 0 {
                parsed.output.extend(self.pending.drain(..start));
            }
            let Some((payload_end, sequence_end)) = terminator(&self.pending) else {
                if self.pending.len() > MAX_OSC_BYTES {
                    parsed.output.append(&mut self.pending);
                }
                break;
            };

            let sequence: Vec<u8> = self.pending.drain(..sequence_end).collect();
            let payload = &sequence[OSC_PREFIX.len()..payload_end];
            if let Some(signal) = parse_signal(payload) {
                parsed.signals.push(signal);
            }
            parsed.output.extend(sequence);
        }

        parsed
    }

    /// Release an incomplete trailing sequence unchanged when the stream ends.
    pub fn finish(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.pending)
    }
}

fn terminator(bytes: &[u8]) -> Option<(usize, usize)> {
    for index in OSC_PREFIX.len()..bytes.len() {
        if bytes[index] == BEL {
            return Some((index, index + 1));
        }
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
            return Some((index, index + 2));
        }
    }
    None
}

fn parse_signal(payload: &[u8]) -> Option<ShellSignal> {
    let text = std::str::from_utf8(payload).ok()?;
    match text {
        "133;A" => Some(ShellSignal::PromptStart),
        "133;B" => Some(ShellSignal::CommandStart),
        "133;C" => Some(ShellSignal::OutputStart),
        _ => {
            if let Some(exit) = text.strip_prefix("133;D;") {
                return exit.parse().ok().map(ShellSignal::CommandEnd);
            }
            if let Some(url) = text.strip_prefix("7;file://") {
                let path_start = url.find('/').unwrap_or(url.len());
                return percent_decode(&url[path_start..]).map(ShellSignal::Cwd);
            }
            text.strip_prefix("7420;cmd=")
                .and_then(decode_base64)
                .and_then(|bytes| String::from_utf8(bytes).ok())
                .map(ShellSignal::CommandText)
        }
    }
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex(*bytes.get(index + 1)?)?;
            let low = hex(*bytes.get(index + 2)?)?;
            decoded.push(high * 16 + low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn decode_base64(value: &str) -> Option<Vec<u8>> {
    let input: Vec<u8> = value
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if input.is_empty() {
        return Some(Vec::new());
    }
    if !input.len().is_multiple_of(4) {
        return None;
    }
    let mut decoded = Vec::with_capacity(input.len() / 4 * 3);
    for chunk in input.chunks_exact(4) {
        let a = base64_value(chunk[0])?;
        let b = base64_value(chunk[1])?;
        let c = if chunk[2] == b'=' {
            0
        } else {
            base64_value(chunk[2])?
        };
        let d = if chunk[3] == b'=' {
            0
        } else {
            base64_value(chunk[3])?
        };
        decoded.push((a << 2) | (b >> 4));
        if chunk[2] != b'=' {
            decoded.push((b << 4) | (c >> 2));
        }
        if chunk[3] != b'=' {
            decoded.push((c << 6) | d);
        }
    }
    Some(decoded)
}

fn base64_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_supported_sequence() {
        let bytes = concat!(
            "\x1b]133;A\x07",
            "\x1b]133;B\x1b\\",
            "\x1b]7420;cmd=cHl0aG9uIHRyYWluLnB5\x07",
            "\x1b]133;C\x07",
            "\x1b]7;file://host/tmp/my%20project\x07",
            "\x1b]133;D;130\x07"
        )
        .as_bytes();
        let parsed = OscParser::new().push(bytes);

        assert_eq!(parsed.output, bytes);
        assert_eq!(
            parsed.signals,
            vec![
                ShellSignal::PromptStart,
                ShellSignal::CommandStart,
                ShellSignal::CommandText("python train.py".into()),
                ShellSignal::OutputStart,
                ShellSignal::Cwd("/tmp/my project".into()),
                ShellSignal::CommandEnd(130),
            ]
        );
    }

    #[test]
    fn sequence_split_at_every_byte_is_recognised_and_preserved() {
        let sequence = b"before\x1b]7420;cmd=ZWNobyByZWFkeQ==\x1b\\after";
        for split in 0..=sequence.len() {
            let mut parser = OscParser::new();
            let first = parser.push(&sequence[..split]);
            let second = parser.push(&sequence[split..]);
            let output = [first.output, second.output, parser.finish()].concat();
            let signals = [first.signals, second.signals].concat();
            assert_eq!(output, sequence, "split at {split}");
            assert_eq!(
                signals,
                vec![ShellSignal::CommandText("echo ready".into())],
                "split at {split}"
            );
        }
    }

    #[test]
    fn unknown_osc_and_ordinary_text_pass_through() {
        let bytes = b"ordinary\x1b]999;untrusted\x07 text";
        let parsed = OscParser::new().push(bytes);
        assert_eq!(parsed.output, bytes);
        assert!(parsed.signals.is_empty());
    }

    #[test]
    fn overlong_unterminated_osc_does_not_hold_later_output() {
        let mut bytes = b"\x1b]7420;cmd=".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', MAX_OSC_BYTES));
        bytes.extend_from_slice(b"visible");
        let parsed = OscParser::new().push(&bytes);
        assert_eq!(parsed.output, bytes);
        assert!(parsed.signals.is_empty());
    }

    #[test]
    fn incomplete_sequence_is_released_on_finish() {
        let bytes = b"text\x1b]133;D;0";
        let mut parser = OscParser::new();
        let parsed = parser.push(bytes);
        assert_eq!(parsed.output, b"text");
        assert_eq!(parser.finish(), b"\x1b]133;D;0");
    }
}
