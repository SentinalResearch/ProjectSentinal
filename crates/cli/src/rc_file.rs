//! Byte-exact insertion and removal of the Sentinal block in a shell rc file.
//!
//! The file is treated as opaque bytes (any encoding). The block is always
//! whole lines at the end of the file. When the original file did not end in
//! a newline, the begin marker records that the separating newline was added,
//! so uninstall restores the original bytes exactly.

/// Begin marker line.
pub const BEGIN: &str = "# >>> sentinal hook >>>";
/// Begin marker line when a final newline was added to the original file.
pub const BEGIN_ADDED_NEWLINE: &str = "# >>> sentinal hook >>> (newline added)";
/// End marker line.
pub const END: &str = "# <<< sentinal hook <<<";

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
}

/// Return the file with the block containing `body` installed; unchanged
/// bytes (equal to `original`) mean it was already installed.
pub fn install(original: &[u8], body: &str) -> Result<Vec<u8>, RcError> {
    if contains(original, b"\r\n") {
        return Err(RcError::Crlf);
    }
    let body = with_final_newline(body);
    if let Some(block) = locate(original)? {
        if &original[block.body_start..block.body_end] == body.as_bytes() {
            return Ok(original.to_vec());
        }
        let mut updated = original[..block.body_start].to_vec();
        updated.extend_from_slice(body.as_bytes());
        updated.extend_from_slice(&original[block.body_end..]);
        return Ok(updated);
    }
    let mut updated = original.to_vec();
    let begin = if original.is_empty() || original.ends_with(b"\n") {
        BEGIN
    } else {
        updated.push(b'\n');
        BEGIN_ADDED_NEWLINE
    };
    updated.extend_from_slice(format!("{begin}\n{body}{END}\n").as_bytes());
    Ok(updated)
}

/// Return the file with the block removed; unchanged bytes mean none was present.
pub fn uninstall(original: &[u8]) -> Result<Vec<u8>, RcError> {
    let Some(block) = locate(original)? else {
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

/// Whether `original` contains a well-formed block.
pub fn is_installed(original: &[u8]) -> Result<bool, RcError> {
    locate(original).map(|block| block.is_some())
}

fn locate(bytes: &[u8]) -> Result<Option<Block>, RcError> {
    let mut begin: Option<(usize, usize, bool, usize)> = None;
    let mut found: Option<Block> = None;
    let mut offset = 0;
    for (index, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let number = index + 1;
        let text = line.strip_suffix(b"\n").unwrap_or(line);
        let next = offset + line.len();
        if contains(text, b">>> sentinal hook") {
            if text != BEGIN.as_bytes() && text != BEGIN_ADDED_NEWLINE.as_bytes() {
                return Err(RcError::Malformed(format!(
                    "line {number} is an edited begin marker"
                )));
            }
            if begin.is_some() || found.is_some() {
                return Err(RcError::Malformed(format!(
                    "line {number} is a second begin marker"
                )));
            }
            begin = Some((offset, next, text == BEGIN_ADDED_NEWLINE.as_bytes(), number));
        } else if contains(text, b"<<< sentinal hook") {
            if text != END.as_bytes() {
                return Err(RcError::Malformed(format!(
                    "line {number} is an edited end marker"
                )));
            }
            let Some((start, body_start, added_newline, _)) = begin.take() else {
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
