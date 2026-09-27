//! Scrollback behaviour tests.

use daemon::scrollback::Scrollback;

#[test]
fn scrollback_keeps_only_the_last_n_lines() {
    let mut scrollback = Scrollback::new(3);

    scrollback.push(b"one\ntwo\n");
    scrollback.push(b"three\nfour\n");

    assert_eq!(scrollback.lines(), vec!["two", "three", "four"]);
}

#[test]
fn scrollback_joins_partial_lines_across_chunks() {
    let mut scrollback = Scrollback::new(3);

    scrollback.push(b"hel");
    scrollback.push(b"lo\nworld");

    assert_eq!(scrollback.lines(), vec!["hello", "world"]);
}

#[test]
fn zero_capacity_scrollback_keeps_no_lines() {
    let mut scrollback = Scrollback::new(0);

    scrollback.push(b"one\ntwo");

    assert!(scrollback.lines().is_empty());
}
