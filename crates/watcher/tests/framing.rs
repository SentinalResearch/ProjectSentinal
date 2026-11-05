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

#[test]
fn a_prompt_waiting_on_the_line_is_offered_once_without_a_newline() {
    // Real prompts leave the cursor on the line: no newline ever arrives.
    let mut framer = LineFramer::default();
    assert!(framer.push(b"Publish the results? [y/N] ").is_empty());
    assert_eq!(
        framer.new_pending_prompt().as_deref(),
        Some("Publish the results? [y/N] ")
    );
    assert_eq!(framer.new_pending_prompt(), None, "offered once");

    // The answer's echo is no longer a prompt; the finished line is a normal line.
    framer.push(b"y");
    assert_eq!(framer.new_pending_prompt(), None);
    assert_eq!(framer.push(b"\n"), vec!["Publish the results? [y/N] y"]);

    // The same question later is new again.
    framer.push(b"Publish the results? [y/N] ");
    assert!(framer.new_pending_prompt().is_some());
}

#[test]
fn ordinary_unfinished_output_is_not_a_prompt() {
    let mut framer = LineFramer::default();
    for text in [&b"Downloading 40%"[..], b"$ ", b"building model"] {
        framer.reset();
        framer.push(text);
        assert_eq!(framer.new_pending_prompt(), None);
    }
}
