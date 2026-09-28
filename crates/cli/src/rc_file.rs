//! Byte-exact insertion and removal of Sentinal blocks in a shell rc file.
//!
//! This is the only rc editor: `sentinal hook` and `sentinal auto` use it with
//! different marker names. The file is treated as opaque bytes (any
//! encoding) and blocks are whole lines. A block appended to a file without a
//! final newline records that the separating newline was added, so removal
//! restores the original bytes exactly.

/// Begin marker line of the hook block.
pub const BEGIN: &str = "# >>> sentinal hook >>>";
/// Hook begin marker when a final newline was added to the original file.
pub const BEGIN_ADDED_NEWLINE: &str = "# >>> sentinal hook >>> (newline added)";
/// End marker line of the hook block.
pub const END: &str = "# <<< sentinal hook <<<";

/// Names one kind of Sentinal block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Markers {
    name: &'static str,
}

impl Markers {
    /// The `sentinal hook` block.
    pub const HOOK: Markers = Markers { name: "hook" };
    /// The `sentinal auto` block.
    pub const AUTO: Markers = Markers { name: "auto" };

    /// Begin marker line.
    pub fn begin(self) -> String {
        format!("# >>> sentinal {} >>>", self.name)
    }

    /// Begin marker line recording an added final newline.
    pub fn begin_added_newline(self) -> String {
        format!("# >>> sentinal {} >>> (newline added)", self.name)
    }

    /// End marker line.
    pub fn end(self) -> String {
        format!("# <<< sentinal {} <<<", self.name)
    }
}

/// Where a new block goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Before everything else (runs before the rest of the rc file).
    Start,
    /// After everything else.
    End,
}

/// 1-based line numbers of an existing block, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockLines {
    /// Begin marker line.
    pub first: usize,
    /// End marker line.
    pub last: usize,
}

/// Why an rc file cannot be edited safely; the file is left untouched.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RcError {
    /// Shells reject carriage returns in rc files; do not add to the damage.
    #[error("the file uses CRLF line endings, which zsh, bash, and fish do not accept; convert it to LF first")]
    Crlf,
    /// Marker lines are missing, repeated, edited, nested, or out of order.
    #[error("the Sentinal block is malformed ({0}); fix or remove it by hand")]
    Malformed(String),
}

/// Where an existing block sits in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Block {
    /// Offset of the begin marker line.
    start: usize,
    /// Offset of the first body byte (after the begin line).
    body_start: usize,
    /// Offset of the end marker line.
    body_end: usize,
    /// Offset just past the end marker line (including its newline, if any).
    end: usize,
    /// The begin marker recorded an added separating newline.
    added_newline: bool,
    /// Line numbers for reporting.
    lines: BlockLines,
}

/// Return the file with the hook block containing `body` installed; unchanged
/// bytes (equal to `original`) mean it was already installed.
pub fn install(original: &[u8], body: &str) -> Result<Vec<u8>, RcError> {
    install_block(original, body, Markers::HOOK, Placement::End)
}

/// Return the file with the hook block removed.
pub fn uninstall(original: &[u8]) -> Result<Vec<u8>, RcError> {
    uninstall_block(original, Markers::HOOK)
}

/// Install a `markers` block with `body`, updating an existing one in place.
pub fn install_block(
    original: &[u8],
    body: &str,
    markers: Markers,
    placement: Placement,
) -> Result<Vec<u8>, RcError> {
    if contains(original, b"\r\n") {
        return Err(RcError::Crlf);
    }
    let body = with_final_newline(body);
    if let Some(block) = locate(original, markers)? {
        if &original[block.body_start..block.body_end] == body.as_bytes() {
            return Ok(original.to_vec());
        }
        let mut updated = original[..block.body_start].to_vec();
        updated.extend_from_slice(body.as_bytes());
        updated.extend_from_slice(&original[block.body_end..]);
        return Ok(updated);
    }
    let end = markers.end();
    if placement == Placement::Start {
        let mut updated = format!("{}\n{body}{end}\n", markers.begin()).into_bytes();
        updated.extend_from_slice(original);
        return Ok(updated);
    }
    let mut updated = original.to_vec();
    let begin = if original.is_empty() || original.ends_with(b"\n") {
        markers.begin()
    } else {
        updated.push(b'\n');
        markers.begin_added_newline()
    };
    updated.extend_from_slice(format!("{begin}\n{body}{end}\n").as_bytes());
    Ok(updated)
}

/// Remove a `markers` block; unchanged bytes mean none was present.
pub fn uninstall_block(original: &[u8], markers: Markers) -> Result<Vec<u8>, RcError> {
    let Some(block) = locate(original, markers)? else {
        return Ok(original.to_vec());
    };
    let mut start = block.start;
    if block.added_newline {
        if start == 0 || original[start - 1] != b'\n' {
            return Err(RcError::Malformed(
                "the newline recorded before the begin marker is missing".into(),
            ));
        }
        start -= 1;
    }
    let mut updated = original[..start].to_vec();
    updated.extend_from_slice(&original[block.end..]);
    Ok(updated)
}

/// Line numbers of a well-formed `markers` block, if present.
pub fn find_block(original: &[u8], markers: Markers) -> Result<Option<BlockLines>, RcError> {
    Ok(locate(original, markers)?.map(|block| block.lines))
}

/// Whether `original` contains a well-formed hook block.
pub fn is_installed(original: &[u8]) -> Result<bool, RcError> {
    locate(original, Markers::HOOK).map(|block| block.is_some())
}

fn locate(bytes: &[u8], markers: Markers) -> Result<Option<Block>, RcError> {
    let (begin_text, begin_added, end_text) = (
        markers.begin(),
        markers.begin_added_newline(),
        markers.end(),
    );
    let begin_probe = format!(">>> sentinal {}", markers.name);
    let end_probe = format!("<<< sentinal {}", markers.name);
    let mut begin: Option<(usize, usize, bool, usize)> = None;
    let mut found: Option<Block> = None;
    let mut offset = 0;
    for (index, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let number = index + 1;
        let text = line.strip_suffix(b"\n").unwrap_or(line);
        let next = offset + line.len();
        if contains(text, begin_probe.as_bytes()) {
            if text != begin_text.as_bytes() && text != begin_added.as_bytes() {
                return Err(RcError::Malformed(format!(
                    "line {number} is an edited begin marker"
                )));
            }
            if begin.is_some() || found.is_some() {
                return Err(RcError::Malformed(format!(
                    "line {number} is a second begin marker"
                )));
            }
            begin = Some((offset, next, text == begin_added.as_bytes(), number));
        } else if contains(text, end_probe.as_bytes()) {
            if text != end_text.as_bytes() {
                return Err(RcError::Malformed(format!(
                    "line {number} is an edited end marker"
                )));
            }
            let Some((start, body_start, added_newline, first)) = begin.take() else {
                return Err(RcError::Malformed(format!(
                    "line {number} ends a block that never began"
                )));
            };
            found = Some(Block {
                start,
                body_start,
                body_end: offset,
                end: next,
                added_newline,
                lines: BlockLines {
                    first,
                    last: number,
                },
            });
        }
        offset = next;
    }
    if let Some((_, _, _, number)) = begin {
        return Err(RcError::Malformed(format!(
            "the block begun on line {number} never ends"
        )));
    }
    Ok(found)
}

fn with_final_newline(body: &str) -> String {
    if body.is_empty() || body.ends_with('\n') {
        body.to_string()
    } else {
        format!("{body}\n")
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}
