//! `sentinal update` restarts sentinald, which ends every live terminal on
//! this Machine (until Reliability R1 keeps them). Say how many and ask.

use protocol::{CaptureLevel, Session};
use std::io::{BufRead, Read, Write};

/// Live terminals a daemon restart would end: FULL Sessions not yet finished.
pub(crate) fn live_terminals(sessions: &[Session]) -> usize {
    sessions
        .iter()
        .filter(|session| session.capture == CaptureLevel::Full && !session.state.is_finished())
        .count()
}

/// The warning for `count` live terminals on `machine`, or `None` for none.
pub(crate) fn warning(count: usize, machine: &str, inside_session: bool) -> Option<String> {
    if count == 0 {
        return None;
    }
    let terminals = if count == 1 { "terminal" } else { "terminals" };
    let this_one = if inside_session {
        ", including this one. Run the update from a terminal outside Sentinal to keep this one"
    } else {
        ""
    };
    Some(format!(
        "This ends {count} live {terminals} on {machine}{this_one}."
    ))
}

/// What to do before installing.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    /// Nothing would end, or the owner already agreed.
    Proceed,
    /// Ask on the terminal.
    Ask(String),
    /// Not interactive and not confirmed: stop with this message.
    Refuse(String),
}

pub(crate) fn decide(warning: Option<String>, yes: bool, interactive: bool) -> Decision {
    match warning {
        None => Decision::Proceed,
        Some(_) if yes => Decision::Proceed,
        Some(text) if interactive => Decision::Ask(text),
        Some(text) => Decision::Refuse(format!(
            "{text} Re-run in a terminal to confirm, or pass --yes."
        )),
    }
}

/// Ask `question`; only "y" or "yes" agrees.
pub(crate) fn confirm(question: &str, input: &mut impl BufRead, out: &mut dyn Write) -> bool {
    let _ = write!(out, "{question} Update anyway? [y/N] ");
    let _ = out.flush();
    let mut answer = String::new();
    let _ = input.by_ref().take(64).read_line(&mut answer);
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::{confirm, decide, warning, Decision};

    #[test]
    fn the_warning_counts_the_terminals_it_ends() {
        assert_eq!(warning(0, "pop-os", false), None);
        assert_eq!(
            warning(1, "pop-os", false).unwrap(),
            "This ends 1 live terminal on pop-os."
        );
        assert_eq!(
            warning(3, "pop-os", true).unwrap(),
            "This ends 3 live terminals on pop-os, including this one. Run the update from a \
             terminal outside Sentinal to keep this one."
        );
    }

    #[test]
    fn live_terminals_need_a_yes() {
        let text = || warning(2, "mini", false);
        assert_eq!(decide(None, false, false), Decision::Proceed);
        assert_eq!(decide(text(), true, false), Decision::Proceed);
        assert_eq!(decide(text(), false, true), Decision::Ask(text().unwrap()));
        assert!(matches!(
            decide(text(), false, false),
            Decision::Refuse(message) if message.ends_with("or pass --yes.")
        ));
    }

    #[test]
    fn only_yes_confirms() {
        for (answer, agreed) in [
            ("y\n", true),
            ("YES\n", true),
            ("\n", false),
            ("no\n", false),
        ] {
            let mut out = Vec::new();
            assert_eq!(
                confirm("Q.", &mut answer.as_bytes(), &mut out),
                agreed,
                "{answer:?}"
            );
            assert_eq!(String::from_utf8(out).unwrap(), "Q. Update anyway? [y/N] ");
        }
    }
}
