//! The terminal engine: the only file in the workspace that names
//! `libghostty-vt` (T2.1). Its handles are neither `Send` nor `Sync`, so an
//! [`Engine`] lives and dies on its Session's screen thread.

use ghostty_vt::fmt::{Format, Formatter, FormatterOptions};
use ghostty_vt::screen::Screen as ActiveScreen;
use ghostty_vt::selection::Selection;
use ghostty_vt::terminal::{Options, Point, PointCoordinate, Terminal};

/// An error from the engine, as text: callers log it and fail open.
pub(crate) type EngineError = String;

/// Upper bound on the bytes of history kept, whatever the line limit.
const MAX_HISTORY_BYTES: usize = 64 * 1024 * 1024;
/// Leave the alternate screen (the program's own full-screen view).
const LEAVE_ALTERNATE: &[u8] = b"\x1b[?1049l";
/// Erase the history (ED 3).
const ERASE_HISTORY: &[u8] = b"\x1b[3J";
/// Cell size reported to programs that ask (CSI 14/16 t). Nothing draws
/// pixels here; a typical monospace cell.
const CELL_WIDTH_PX: u32 = 8;
const CELL_HEIGHT_PX: u32 = 16;

/// The visible rows of the screen, for the watcher.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScreenRows {
    /// One string per row, top to bottom, without trailing spaces.
    pub rows: Vec<String>,
    /// A full-screen program has switched to the alternate screen.
    pub alternate: bool,
    /// Cursor column and row, from 0.
    pub cursor: (u16, u16),
}

/// One Session's screen.
pub(crate) struct Engine {
    terminal: Terminal,
}

fn error(error: ghostty_vt::Error) -> EngineError {
    error.to_string()
}

impl Engine {
    /// A blank screen of `cols` × `rows` that keeps about `history_lines`
    /// lines that scrolled off the top.
    pub(crate) fn new(cols: u16, rows: u16, history_lines: usize) -> Result<Self, EngineError> {
        let mut terminal = Terminal::new(Options {
            cols: cols.max(1),
            rows: rows.max(1),
            max_scrollback: if history_lines == 0 {
                0
            } else {
                MAX_HISTORY_BYTES
            },
        })
        .map_err(error)?;
        terminal
            .set_scrollback_max_lines(Some(history_lines))
            .map_err(error)?;
        Ok(Self { terminal })
    }

    /// Draw `bytes`. Answers to queries and other effects (titles, bells,
    /// clipboard writes) are dropped: viewers answer for themselves.
    pub(crate) fn feed(&mut self, bytes: &[u8]) {
        self.terminal.vt_write(bytes);
        self.terminal.events().for_each(drop);
    }

    /// Change the size; lines that wrapped reflow to the new width.
    pub(crate) fn resize(&mut self, cols: u16, rows: u16) -> Result<(), EngineError> {
        self.terminal
            .resize(cols.max(1), rows.max(1), CELL_WIDTH_PX, CELL_HEIGHT_PX)
            .map_err(error)?;
        self.terminal.events().for_each(drop);
        Ok(())
    }

    /// The parser is between sequences: nothing it has read is unfinished.
    #[cfg(test)]
    pub(crate) fn at_sequence_boundary(&self) -> bool {
        self.terminal.is_vt_ground().unwrap_or(false)
    }

    fn alternate(&self) -> Result<bool, EngineError> {
        Ok(self.terminal.active_screen().map_err(error)? == ActiveScreen::Alternate)
    }

    /// Bytes that draw this screen on a blank viewer of the same size: the
    /// history first when `history_lines` is set (about that many lines,
    /// scrolled into the viewer's own scrollback), then the screen, with
    /// colours, modes and the cursor. On the alternate screen the main screen
    /// is drawn first, so the viewer has it back when the program leaves.
    pub(crate) fn snapshot(&self, history_lines: Option<usize>) -> Result<Vec<u8>, EngineError> {
        let alternate = self.alternate()?;
        let history = self.terminal.scrollback_rows().map_err(error)?;
        let trimmed = match history_lines {
            Some(lines) => history <= lines,
            None => history == 0,
        };
        if !alternate && trimmed {
            return vt(&self.terminal);
        }
        let mut main = self.copy()?;
        if alternate {
            main.feed(LEAVE_ALTERNATE);
        }
        match history_lines {
            Some(lines) if history > lines => main = self.trimmed(main, alternate, lines)?,
            Some(_) => {}
            None => main.feed(ERASE_HISTORY),
        }
        let mut out = vt(&main.terminal)?;
        if alternate {
            // Starts by entering the alternate screen itself.
            out.extend(vt(&self.terminal)?);
        }
        Ok(out)
    }

    /// The visible rows, for the watcher.
    pub(crate) fn rows(&self) -> Result<ScreenRows, EngineError> {
        let cols = self.terminal.cols().map_err(error)?;
        let rows = self.terminal.rows().map_err(error)?;
        let start = self
            .terminal
            .grid_ref(Point::Active(PointCoordinate { x: 0, y: 0 }))
            .map_err(error)?;
        let end = self
            .terminal
            .grid_ref(Point::Active(PointCoordinate {
                x: cols.saturating_sub(1),
                y: u32::from(rows.saturating_sub(1)),
            }))
            .map_err(error)?;
        let selection = Selection::new(start, end, false);
        let options = FormatterOptions::new()
            .with_format(Format::Plain)
            .with_selection(&selection);
        let text = format(&self.terminal, options)?;
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|row| row.trim_end().to_string())
            .collect();
        lines.resize(usize::from(rows), String::new());
        Ok(ScreenRows {
            rows: lines,
            alternate: self.alternate()?,
            cursor: (
                self.terminal.cursor_x().map_err(error)?,
                self.terminal.cursor_y().map_err(error)?,
            ),
        })
    }

    /// History and screen as text lines, oldest first, without the blank
    /// rows below the last line written.
    pub(crate) fn lines(&self) -> Result<Vec<String>, EngineError> {
        let options = FormatterOptions::new()
            .with_format(Format::Plain)
            .with_trim(true);
        let text = format(&self.terminal, options)?;
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|row| row.trim_end().to_string())
            .collect();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        Ok(lines)
    }

    /// `main` with about `lines` of history. libghostty drops history a
    /// page at a time, which can leave far fewer lines than asked; a second
    /// copy asks for more by what the first fell short.
    fn trimmed(&self, mut main: Self, alternate: bool, lines: usize) -> Result<Self, EngineError> {
        main.terminal
            .set_scrollback_max_lines(Some(lines))
            .map_err(error)?;
        let kept = main.terminal.scrollback_rows().map_err(error)?;
        if kept * 10 >= lines * 9 {
            return Ok(main);
        }
        let mut again = self.copy()?;
        if alternate {
            again.feed(LEAVE_ALTERNATE);
        }
        again
            .terminal
            .set_scrollback_max_lines(Some(lines + (lines - kept)))
            .map_err(error)?;
        Ok(again)
    }

    /// An independent copy of the whole state, both screens and history.
    fn copy(&self) -> Result<Self, EngineError> {
        let encoded = ghostty_vt::snapshot::encode(&self.terminal).map_err(error)?;
        let terminal = ghostty_vt::snapshot::Decoder::from_slice(&encoded)
            .decode()
            .map_err(error)?;
        Ok(Self { terminal })
    }
}

fn format(terminal: &Terminal, options: FormatterOptions<'_, '_>) -> Result<String, EngineError> {
    let mut formatter = Formatter::new(terminal, options).map_err(error)?;
    let bytes = formatter.format_alloc().map_err(error)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The active screen (and its history) as VT bytes.
fn vt(terminal: &Terminal) -> Result<Vec<u8>, EngineError> {
    let options = FormatterOptions::new()
        .with_format(Format::Vt)
        .with_cursor(true)
        .with_style(true)
        .with_modes(true)
        .with_scrolling_region(true)
        .with_tabstops(true)
        .with_charsets(true)
        .with_hyperlink(true);
    let mut formatter = Formatter::new(terminal, options).map_err(error)?;
    Ok(formatter.format_alloc().map_err(error)?.to_vec())
}
