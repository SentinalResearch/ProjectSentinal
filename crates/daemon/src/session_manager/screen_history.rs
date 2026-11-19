//! The attach snapshot with the Session's scrollback in front of the screen,
//! so a viewer that asks for history can scroll back through what it missed.
//!
//! Each scrollback row is written as its own line, oldest first. Then the
//! viewer's lines above the cursor are scrolled into its own scrollback (one
//! line feed fewer than the screen has rows: no blank line ever enters it),
//! and the screen is redrawn exactly as a plain snapshot would. A full-screen
//! program on the alternate screen has no scrollback to give, so it gets the
//! plain snapshot.

/// Scrollback rows, oldest first, then the current screen.
pub(crate) fn history_then_screen(screen: &vt100::Screen) -> Vec<u8> {
    if screen.alternate_screen() {
        return screen.state_formatted();
    }
    let (rows, cols) = screen.size();
    let page = usize::from(rows.max(1));
    let mut view = screen.clone();
    view.set_scrollback(usize::MAX);
    let mut offset = view.scrollback();
    let mut out = Vec::new();
    while offset > 0 {
        view.set_scrollback(offset);
        let take = offset.min(page);
        for row in view.rows_formatted(0, cols).take(take) {
            out.extend_from_slice(&row);
            out.extend_from_slice(b"\x1b[m\r\n");
        }
        offset -= take;
    }
    out.extend(std::iter::repeat_n(b'\n', page - 1));
    out.extend(screen.state_formatted());
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// Every line the viewer can scroll to, oldest first, then the screen.
    fn all_lines(screen: &vt100::Screen) -> Vec<String> {
        let mut view = screen.clone();
        view.set_scrollback(usize::MAX);
        let mut offset = view.scrollback();
        let (rows, cols) = screen.size();
        let mut lines = Vec::new();
        while offset > 0 {
            view.set_scrollback(offset);
            let take = offset.min(usize::from(rows));
            lines.extend(view.rows(0, cols).take(take));
            offset -= take;
        }
        lines.extend(screen.rows(0, cols));
        lines
    }

    fn replay(bytes: &[u8], rows: u16) -> vt100::Parser {
        let mut viewer = vt100::Parser::new(rows, 30, 1_000);
        viewer.process(bytes);
        viewer
    }

    #[test]
    fn a_viewer_gets_every_scrollback_line_then_the_same_screen() {
        let mut session = vt100::Parser::new(4, 30, 1_000);
        for line in 1..=12 {
            session.process(format!("line {line}\r\n").as_bytes());
        }
        session.process(b"$ ");

        let viewer = replay(&history_then_screen(session.screen()), 4);

        assert_eq!(all_lines(viewer.screen()), all_lines(session.screen()));
        assert_eq!(viewer.screen().contents(), session.screen().contents());
        assert_eq!(
            viewer.screen().cursor_position(),
            session.screen().cursor_position()
        );
    }

    #[test]
    fn a_short_session_puts_no_blank_lines_into_the_viewers_scrollback() {
        let mut session = vt100::Parser::new(5, 30, 1_000);
        session.process(b"one\r\ntwo\r\n$ ");

        let viewer = replay(&history_then_screen(session.screen()), 5);

        assert_eq!(all_lines(viewer.screen()), all_lines(session.screen()));
        let mut scrolled = viewer.screen().clone();
        scrolled.set_scrollback(usize::MAX);
        assert_eq!(scrolled.scrollback(), 0);
    }

    #[test]
    fn earlier_viewer_lines_move_into_its_scrollback_instead_of_being_cleared() {
        let mut session = vt100::Parser::new(4, 30, 1_000);
        session.process(b"fresh shell\r\n$ ");
        let mut viewer = vt100::Parser::new(4, 30, 1_000);
        viewer.process(b"part one a\r\npart one b\r\n");

        viewer.process(&history_then_screen(session.screen()));

        let lines = all_lines(viewer.screen());
        let text: Vec<&str> = lines.iter().map(|line| line.trim_end()).collect();
        assert_eq!(&text[..2], ["part one a", "part one b"]);
        assert_eq!(viewer.screen().contents(), session.screen().contents());
    }

    #[test]
    fn colours_survive_in_the_scrollback() {
        let mut session = vt100::Parser::new(2, 30, 1_000);
        session.process(b"\x1b[31mred\x1b[m\r\nplain\r\nmore\r\n");

        let viewer = replay(&history_then_screen(session.screen()), 2);

        let mut scrolled = viewer.screen().clone();
        scrolled.set_scrollback(usize::MAX);
        let cell = scrolled.cell(0, 0).unwrap();
        assert_eq!(cell.contents(), "r");
        assert_eq!(cell.fgcolor(), vt100::Color::Idx(1));
    }

    #[test]
    fn a_full_screen_program_gets_the_plain_snapshot() {
        let mut session = vt100::Parser::new(4, 30, 1_000);
        session.process(b"a\r\nb\r\nc\r\nd\r\ne\r\n\x1b[?1049hTOP");

        assert_eq!(
            history_then_screen(session.screen()),
            session.screen().state_formatted()
        );
    }
}
