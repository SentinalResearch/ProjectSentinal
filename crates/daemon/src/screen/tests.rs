#![allow(clippy::unwrap_used)]

use super::*;

fn size(cols: u16, rows: u16) -> ScreenSize {
    ScreenSize {
        cols,
        rows,
        history_lines: 1_000,
        attach_history_lines: 100,
    }
}

fn screen(cols: u16, rows: u16) -> (Screen, broadcast::Sender<Vec<u8>>) {
    let (output, _) = broadcast::channel(1_024);
    (
        Screen::spawn("test", size(cols, rows), output.clone()),
        output,
    )
}

/// What a viewer that started from `snapshot` and then got `rest` shows.
fn viewer(snapshot: &[u8], rest: &[u8], cols: u16, rows: u16) -> Vec<String> {
    render_lines(&[snapshot, rest].concat(), cols, rows, 1_000)
}

#[test]
fn output_is_drawn_then_published_in_order() {
    let (screen, output) = screen(20, 4);
    let mut live = output.subscribe();
    screen.feed(b"one ".to_vec());
    screen.feed(b"two".to_vec());
    assert_eq!(screen.rows().unwrap().rows[0], "one two");
    // Published only once drawn: the rows above were answered after both.
    assert_eq!(live.try_recv().unwrap(), b"one ");
    assert_eq!(live.try_recv().unwrap(), b"two");
}

#[test]
fn a_snapshot_and_its_receiver_split_the_output_exactly() {
    let (screen, _) = screen(20, 4);
    screen.feed(b"before".to_vec());
    let (snapshot, mut after) = screen.snapshot(false);
    screen.feed(b" after".to_vec());
    assert!(String::from_utf8_lossy(&snapshot).contains("before"));
    assert_eq!(after.blocking_recv().unwrap(), b" after");
    assert!(after.try_recv().is_err());
}

#[test]
fn leaving_the_alternate_screen_after_an_attach_brings_back_the_main_screen() {
    // T1.5 left this open: a snapshot carried only the screen on show.
    let (screen, _) = screen(30, 5);
    screen.feed(b"$ ls\r\nnotes.txt\r\n$ htop\r\n".to_vec());
    screen.feed(b"\x1b[?1049h\x1b[2J\x1b[HCPU 42%".to_vec());
    let (snapshot, _) = screen.snapshot(false);
    let rest = b"\x1b[?1049l$ ";
    screen.feed(rest.to_vec());

    let shown = viewer(&snapshot, rest, 30, 5);
    assert_eq!(shown, screen.lines());
    assert_eq!(shown, ["$ ls", "notes.txt", "$ htop", "$"]);
}

#[test]
fn a_viewer_on_the_alternate_screen_sees_the_program_not_the_shell() {
    let (screen, _) = screen(30, 5);
    screen.feed(b"$ htop\r\n\x1b[?1049h\x1b[2J\x1b[HCPU 42%\x1b[3;1Hmem 1G".to_vec());
    let (snapshot, _) = screen.snapshot(true);
    let rows = render_rows(&snapshot, 30, 5);
    assert_eq!(rows, ["CPU 42%", "", "mem 1G", "", ""]);
}

#[test]
fn a_history_snapshot_carries_about_the_configured_lines_and_a_plain_one_none() {
    let (output, _) = broadcast::channel(16);
    let screen = Screen::spawn(
        "test",
        ScreenSize {
            cols: 40,
            rows: 5,
            history_lines: 10_000,
            attach_history_lines: 2_000,
        },
        output,
    );
    let mut bytes = Vec::new();
    for line in 0..6_000 {
        bytes.extend_from_slice(format!("line {line}\r\n").as_bytes());
    }
    for chunk in bytes.chunks(8_192) {
        screen.feed(chunk.to_vec());
    }

    let (with_history, _) = screen.snapshot(true);
    let seen = render_lines(&with_history, 40, 5, 10_000);
    assert_eq!(seen.last().unwrap(), "line 5999");
    // libghostty trims at page granularity: about, never more than asked
    // plus a page, and never all of it.
    assert!(
        (1_800..=2_600).contains(&seen.len()),
        "{} lines of history",
        seen.len()
    );
    assert!(!seen.contains(&"line 0".to_string()));

    let (plain, _) = screen.snapshot(false);
    let seen = render_lines(&plain, 40, 5, 10_000);
    assert_eq!(seen, ["line 5996", "line 5997", "line 5998", "line 5999"]);
}

#[test]
fn colours_and_the_cursor_survive_a_snapshot() {
    let (screen, _) = screen(20, 4);
    screen.feed(b"\x1b[31mred\x1b[0m plain\r\n\x1b[1mbo".to_vec());
    let (snapshot, _) = screen.snapshot(true);
    let text = String::from_utf8_lossy(&snapshot);
    assert!(text.contains("red"), "{text:?}");
    assert!(text.contains("\x1b[38;5;1mred"), "{text:?}");
    // Typing continues where the program left the cursor.
    assert_eq!(
        viewer(&snapshot, b"ld", 20, 4),
        ["red plain", "bold"],
        "{text:?}"
    );
}

#[test]
fn a_resize_reflows_wrapped_lines() {
    let (screen, _) = screen(10, 4);
    screen.feed(b"abcdefghijklmno".to_vec());
    assert_eq!(&screen.rows().unwrap().rows[..2], ["abcdefghij", "klmno"]);
    screen.resize(20, 4);
    assert_eq!(screen.rows().unwrap().rows[0], "abcdefghijklmno");
}

#[test]
fn the_watcher_gets_every_row_and_the_alternate_flag() {
    let (screen, _) = screen(20, 4);
    screen.feed(b"\x1b[3;1Hthird".to_vec());
    let rows = screen.rows().unwrap();
    assert_eq!(rows.rows, ["", "", "third", ""]);
    assert!(!rows.alternate);
    assert_eq!(rows.cursor, (5, 2));
    screen.feed(b"\x1b[?1049h".to_vec());
    assert!(screen.rows().unwrap().alternate);
}

#[test]
fn queries_from_the_program_are_not_answered_and_do_not_pile_up() {
    let (screen, output) = screen(20, 4);
    for _ in 0..10_000 {
        screen.feed(b"\x1b[6n\x1b[c\x1b]0;title\x07\x07".to_vec());
    }
    // Every query drawn (and none queued as an answer) before listening.
    assert!(screen.lines().is_empty());
    let mut live = output.subscribe();
    screen.feed(b"ok".to_vec());
    assert_eq!(screen.rows().unwrap().rows[0], "ok");
    // Viewers still get the queries, to answer as they would.
    assert_eq!(live.try_recv().unwrap(), b"ok");
}

#[test]
fn without_an_engine_viewers_still_get_output() {
    let (output, _) = broadcast::channel(4);
    let screen = Screen::without_engine(output.clone());
    let (snapshot, mut live) = screen.snapshot(true);
    screen.feed(b"still here".to_vec());
    assert!(snapshot.is_empty());
    assert_eq!(live.try_recv().unwrap(), b"still here");
    assert!(screen.rows().is_none());
    assert!(screen.lines().is_empty());
}

#[test]
fn a_viewer_attaching_between_any_two_reads_ends_with_the_daemon_s_screen() {
    use crate::session_manager::SequenceBoundary;
    let mut stream = Vec::new();
    stream.extend_from_slice(b"\x1b]0;build \xe2\x9c\x85 title\x07");
    stream.extend_from_slice(b"\x1b[1;32mOK\x1b[0m compiling \xe6\xbc\xa2\xe5\xad\x97\r\n");
    stream.extend_from_slice(b"\x1b[?1049h\x1b[2J\x1b[H\x1b[38;5;208m\xf0\x9f\xa7\xaa test\x1b[0m");
    stream.extend_from_slice(b"\x1bP+q544e\x1b\\\x1b(B\x1b[5;10Hdone \xc3\xa9t\xc3\xa9\r\n");
    stream.extend_from_slice(b"\x1b[?1049lback on the main screen\r\n\x1b[3mitalic\x1b[23m end");
    for cut in 0..=stream.len() {
        let (screen, _) = screen(60, 12);
        let mut boundary = SequenceBoundary::default();
        screen.feed(boundary.push(&stream[..cut]));
        assert!(screen.at_sequence_boundary(), "cut at byte {cut}");
        let (snapshot, mut live) = screen.snapshot(true);
        screen.feed(boundary.push(&stream[cut..]));
        screen.feed(boundary.finish());
        let daemon = screen.lines();
        let mut rest = Vec::new();
        while let Ok(bytes) = live.try_recv() {
            rest.extend(bytes);
        }
        assert_eq!(
            viewer(&snapshot, &rest, 60, 12),
            daemon,
            "viewer attached at byte {cut} drew a different screen"
        );
    }
}

#[test]
fn recorded_program_output_draws_like_the_program_meant() {
    // The T2 gate ran every recorded log through both engines; these are the
    // screens worth pinning.
    let logs = concat!(env!("CARGO_MANIFEST_DIR"), "/../watcher/tests/logs");
    for (file, shown, overwritten) in [
        // `\r` redraws the bar in place: 11/100 is gone, 12/100 is shown.
        ("training/tqdm.log", "12/100 [00:01<00:08", Some("11/100")),
        (
            "cargo/build_error.log",
            "error: could not compile `watcher`",
            None,
        ),
        ("pytest/fail.log", "1 failed, 2 passed", None),
    ] {
        let raw = std::fs::read(format!("{logs}/{file}")).unwrap();
        let mut bytes = Vec::new();
        for byte in raw {
            if byte == b'\n' {
                bytes.push(b'\r');
            }
            bytes.push(byte);
        }
        let lines = render_lines(&bytes, 120, 40, 10_000);
        assert!(
            lines.iter().any(|line| line.contains(shown)),
            "{file}: {lines:?}"
        );
        if let Some(gone) = overwritten {
            assert!(
                lines.iter().all(|line| !line.contains(gone)),
                "{file}: {lines:?}"
            );
        }
        assert!(
            lines.iter().all(|line| !line.contains('\u{1b}')),
            "{file}: an escape sequence drawn as text"
        );
    }
}

#[test]
fn a_checkpoint_restores_both_screens_and_history_in_a_fresh_screen() {
    // T5: what a keeper keeps across a daemon restart.
    let (screen, _) = screen(30, 5);
    let mut bytes = Vec::new();
    for line in 0..40 {
        bytes.extend_from_slice(format!("line {line}\r\n").as_bytes());
    }
    bytes.extend_from_slice(b"\x1b[?1049h\x1b[2J\x1b[HFULL SCREEN");
    screen.feed(bytes);
    let checkpoint = screen.checkpoint().unwrap();

    let (fresh, _) = self::screen(30, 5);
    assert!(fresh.restore(checkpoint));
    assert_eq!(fresh.rows().unwrap().rows[0], "FULL SCREEN");
    assert!(fresh.rows().unwrap().alternate);
    fresh.feed(b"\x1b[?1049l".to_vec());
    let lines = fresh.lines();
    assert!(lines.contains(&"line 0".to_string()), "{lines:?}");
    assert!(lines.contains(&"line 39".to_string()), "{lines:?}");
    assert!(!fresh.restore(b"not a snapshot".to_vec()));
}
