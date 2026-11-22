//! A CLI viewer that follows the Session's size (T3.5).
//!
//! The daemon owns the size; the viewer the owner last typed in sets it.
//! When this terminal did not set it, the screen arrives at another
//! device's size. This window then:
//!
//! - cuts long lines at its right edge instead of wrapping them, when the
//!   Session is wider (line wrap off after every frame: the daemon's redraw
//!   begins with a full reset, which turns it back on);
//! - says so in one status line: in the first free row below the Session's
//!   screen when there is one, else in the window title.
//!
//! Typing makes this terminal the one that sets the size; the daemon sends
//! the new size and a fresh screen. Pure: bytes in, bytes out.

/// Wrap off: lines are cut at the right edge.
const WRAP_OFF: &[u8] = b"\x1b[?7l";
/// Wrap on, the terminal default.
const WRAP_ON: &[u8] = b"\x1b[?7h";

/// This window and the Session's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SizeView {
    /// This terminal window, columns × rows.
    pub(crate) window: (u16, u16),
    /// The Session's size and whether this viewer set it, once the daemon
    /// has said (an old daemon never does).
    pub(crate) session: Option<(u16, u16, bool)>,
}

impl SizeView {
    /// A window whose Session size is not known yet.
    pub(crate) fn new(window: (u16, u16)) -> Self {
        Self {
            window,
            session: None,
        }
    }

    fn other_device(&self) -> Option<(u16, u16)> {
        match self.session {
            Some((cols, rows, false)) if (cols, rows) != self.window => Some((cols, rows)),
            _ => None,
        }
    }

    /// One line saying the screen is sized for another device, or `None`.
    pub(crate) fn status(&self) -> Option<String> {
        let (cols, rows) = self.other_device()?;
        Some(format!(
            "Sized for another device ({cols}×{rows}) · type here to take over"
        ))
    }

    /// Bytes to write after each frame from the daemon: wrap off and the
    /// status line while sized for another device, nothing otherwise.
    pub(crate) fn after_frame(&self) -> Vec<u8> {
        let Some((cols, rows)) = self.other_device() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if cols > self.window.0 {
            out.extend_from_slice(WRAP_OFF);
        }
        if rows < self.window.1 {
            if let Some(status) = self.status() {
                let line: String = status.chars().take(usize::from(self.window.0)).collect();
                // Save the cursor, draw below the Session's screen, restore.
                out.extend_from_slice(
                    format!("\x1b7\x1b[{};1H\x1b[7m{line}\x1b[0m\x1b[K\x1b8", rows + 1).as_bytes(),
                );
            }
        }
        out
    }

    /// Bytes for a change of size or ownership: the window title (the status
    /// line when there is no free row), and wrap back on when this terminal
    /// set the size.
    pub(crate) fn on_change(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match (self.status(), self.other_device()) {
            (Some(status), Some((_, rows))) if rows >= self.window.1 => {
                out.extend_from_slice(format!("\x1b]0;{status}\x07").as_bytes());
            }
            (None, _) => {
                out.extend_from_slice(WRAP_ON);
                out.extend_from_slice(b"\x1b]0;\x07");
            }
            _ => {}
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(window: (u16, u16), session: (u16, u16, bool)) -> SizeView {
        SizeView {
            window,
            session: Some(session),
        }
    }

    #[test]
    fn the_owner_draws_as_usual() {
        let owner = view((132, 40), (132, 40, true));
        assert_eq!(owner.status(), None);
        assert!(owner.after_frame().is_empty());
        assert_eq!(owner.on_change(), b"\x1b[?7h\x1b]0;\x07");
        // An old daemon never sends a size: nothing changes.
        assert!(SizeView::new((80, 24)).after_frame().is_empty());
    }

    #[test]
    fn a_wider_session_is_cut_at_the_edge_and_said_in_the_title() {
        let laptop_on_phone = view((80, 24), (132, 40, false));
        assert_eq!(laptop_on_phone.after_frame(), WRAP_OFF);
        let change = String::from_utf8(laptop_on_phone.on_change()).unwrap();
        assert!(change.starts_with("\x1b]0;Sized for another device (132×40)"));
    }

    #[test]
    fn a_smaller_session_gets_the_status_line_below_it() {
        let phone_owned = view((132, 40), (44, 20, false));
        let bytes = String::from_utf8(phone_owned.after_frame()).unwrap();
        assert!(!bytes.contains("?7l"), "narrower: nothing to cut");
        assert!(bytes.starts_with("\x1b7\x1b[21;1H\x1b[7mSized for another device (44×20)"));
        assert!(bytes.ends_with("\x1b8"));
        assert!(phone_owned.on_change().is_empty(), "the row says it");
    }

    #[test]
    fn the_status_line_fits_the_window() {
        let narrow = view((20, 30), (100, 10, false));
        let bytes = String::from_utf8(narrow.after_frame()).unwrap();
        let line = bytes
            .split("\x1b[7m")
            .nth(1)
            .unwrap()
            .split("\x1b[0m")
            .next()
            .unwrap();
        assert_eq!(line.chars().count(), 20);
    }
}
