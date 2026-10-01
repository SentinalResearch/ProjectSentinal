//! Streaming output-framing behavior tests.

use watcher::LineFramer;

#[test]
fn strips_ansi_splits_newline_and_carriage_return_and_caps_lines() {
    let mut framer = LineFramer::new(8);
    let lines = framer.push(b"\x1b[31mone\x1b[0m\rtwo\n123456789\n");
    assert_eq!(lines, ["one", "two", "12345678"]);
}

#[test]
fn split_escape_sequences_and_partial_lines_are_preserved_safely() {
    let mut framer = LineFramer::new(8_192);
    assert!(framer.push(b"\x1b[3").is_empty());
    assert_eq!(framer.push(b"2mgreen\x1b[0m\n"), ["green"]);
    assert!(framer.push(b"partial").is_empty());
    assert_eq!(framer.pending(), "partial");
    assert_eq!(framer.finish(), Some("partial".into()));
}
