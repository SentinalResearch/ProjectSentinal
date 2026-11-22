//! Answering a menu by its place in the list (T3.4).
//!
//! Arrow-key menus (Mistral Vibe, Claude Code) cannot be answered by a key
//! named in the question: the keys depend on where the selector is now, and
//! the owner may have moved it since the question was reported. So the
//! daemon computes them at the moment of answering, from the current screen,
//! and refuses when the question or the menu on screen is no longer the one
//! that was shown.

use super::capture::CaptureOperation;
use super::{lock_session, SessionManager, SessionManagerError};
use protocol::{EventId, PromptOption, SessionId, SessionState};

/// Why an answer was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerRefused {
    /// The Session is not waiting for an answer.
    #[error("session is not waiting for an answer")]
    NotWaiting,
    /// A newer question replaced the one shown.
    #[error("the question changed; look again before answering")]
    QuestionChanged,
    /// The menu on screen is not the one reported.
    #[error("the menu on screen changed; answer it in the terminal")]
    MenuChanged,
    /// No option is marked, so arrows have nowhere to start.
    #[error("no option is selected on screen; answer it in the terminal")]
    NothingMarked,
    /// The chosen option is not in the menu.
    #[error("option {0} is not in the menu")]
    NoSuchOption(u8),
}

/// The keys that move the selector from `marked` to `chosen` and press
/// Enter. `application` is the cursor-key mode the program asked for.
pub(crate) fn menu_keys(marked: u8, chosen: u8, application: bool) -> Vec<u8> {
    let (down, up): (&[u8], &[u8]) = if application {
        (b"\x1bOB", b"\x1bOA")
    } else {
        (b"\x1b[B", b"\x1b[A")
    };
    let (arrow, count) = if chosen >= marked {
        (down, chosen - marked)
    } else {
        (up, marked - chosen)
    };
    let mut keys = arrow.repeat(usize::from(count));
    keys.push(b'\r');
    keys
}

/// Check the answer against what was asked and what is on screen now, and
/// return the keys to send.
pub(crate) fn answer_keys(
    asked: Option<&(EventId, Vec<PromptOption>)>,
    event_id: EventId,
    on_screen: Option<(Vec<PromptOption>, Option<u8>)>,
    option: u8,
    application: bool,
) -> Result<Vec<u8>, AnswerRefused> {
    let (asked_id, asked_options) = asked.ok_or(AnswerRefused::NotWaiting)?;
    if *asked_id != event_id {
        return Err(AnswerRefused::QuestionChanged);
    }
    if usize::from(option) >= asked_options.len() {
        return Err(AnswerRefused::NoSuchOption(option));
    }
    let (options, marked) = on_screen.ok_or(AnswerRefused::MenuChanged)?;
    if &options != asked_options {
        return Err(AnswerRefused::MenuChanged);
    }
    let marked = marked.ok_or(AnswerRefused::NothingMarked)?;
    Ok(menu_keys(marked, option, application))
}

impl SessionManager {
    /// `ANSWER_PROMPT`: choose `option` of the menu in Event `event_id`.
    pub fn answer_prompt(
        &self,
        session_id: SessionId,
        event_id: EventId,
        option: u8,
        client_addr: &str,
    ) -> Result<(), SessionManagerError> {
        let handle = self.operation_handle(session_id, CaptureOperation::Input)?;
        if lock_session(&handle)?.state != SessionState::Waiting {
            return Err(AnswerRefused::NotWaiting.into());
        }
        let asked = handle
            .asked
            .lock()
            .map_err(|_| SessionManagerError::Lock {
                resource: "asked question",
                session_id,
            })?
            .clone();
        let screen = handle.screen.rows();
        let application = screen
            .as_ref()
            .is_some_and(|screen| screen.application_cursor);
        let on_screen = screen.and_then(|screen| watcher::menu_on_screen(&screen.rows));
        let keys = answer_keys(asked.as_ref(), event_id, on_screen, option, application)?;
        self.write_input(session_id, &keys, client_addr)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn options(labels: &[&str]) -> Vec<PromptOption> {
        labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                serde_json::from_value(serde_json::json!({
                    "label": label,
                    "keys": format!("{}", index + 1),
                }))
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn keys_move_from_the_marked_row_then_press_enter() {
        assert_eq!(menu_keys(0, 2, false), b"\x1b[B\x1b[B\r");
        assert_eq!(menu_keys(2, 0, false), b"\x1b[A\x1b[A\r");
        assert_eq!(menu_keys(1, 1, false), b"\r");
        assert_eq!(menu_keys(0, 1, true), b"\x1bOB\r");
    }

    #[test]
    fn the_answer_follows_where_the_selector_is_now() {
        let menu = options(&["Yes", "Yes, always", "No"]);
        let asked = (EventId(812), menu.clone());
        // Reported with 1 marked, but the owner moved it to 3 since.
        let keys = answer_keys(Some(&asked), EventId(812), Some((menu, Some(2))), 0, false);
        assert_eq!(keys.unwrap(), b"\x1b[A\x1b[A\r");
    }

    #[test]
    fn a_changed_question_or_menu_is_refused() {
        let menu = options(&["Yes", "No"]);
        let asked = (EventId(812), menu.clone());
        assert_eq!(
            answer_keys(None, EventId(812), Some((menu.clone(), Some(0))), 1, false),
            Err(AnswerRefused::NotWaiting)
        );
        assert_eq!(
            answer_keys(
                Some(&asked),
                EventId(813),
                Some((menu.clone(), Some(0))),
                1,
                false
            ),
            Err(AnswerRefused::QuestionChanged)
        );
        assert_eq!(
            answer_keys(
                Some(&asked),
                EventId(812),
                Some((options(&["Allow", "Deny"]), Some(0))),
                1,
                false
            ),
            Err(AnswerRefused::MenuChanged)
        );
        assert_eq!(
            answer_keys(Some(&asked), EventId(812), None, 1, false),
            Err(AnswerRefused::MenuChanged)
        );
        assert_eq!(
            answer_keys(
                Some(&asked),
                EventId(812),
                Some((menu.clone(), None)),
                1,
                false
            ),
            Err(AnswerRefused::NothingMarked)
        );
        assert_eq!(
            answer_keys(Some(&asked), EventId(812), Some((menu, Some(0))), 5, false),
            Err(AnswerRefused::NoSuchOption(5))
        );
    }
}
