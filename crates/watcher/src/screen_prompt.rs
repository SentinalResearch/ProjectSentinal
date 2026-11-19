//! Questions a program is asking, read from its parsed screen.
//!
//! Full-screen programs (AI coding agents above all) move the cursor and redraw,
//! so their output is not lines: words merge and the same question reads
//! differently each time. The screen is what a person sees, so a menu is found
//! there, for any program, without knowing which one it is:
//!
//! - Claude Code: `Do you want to proceed?` then `❯ 1. Yes`, `  2. No`
//! - Mistral Vibe: `Permission for the … tool`, numbered options, `Enter select`
//! - aider: `Run shell command? (Y)es/(N)o [Yes]:`
//!
//! A question is reported once it has stayed on screen for a moment, and it ends
//! only when it has left the screen: never by a timer or other output, because a
//! program redraws its status line while it waits. After input it ends as soon
//! as it leaves, so the same question asked again is reported again. Terminal
//! text is data.

use crate::agent_cli::{clean, is_hint, letter_choices, option_line, MARKERS};
use protocol::{event::MAX_PROMPT_OPTIONS, EventKind, PromptOption, Timestamp};

/// On screen this long before it is a question, so a half-drawn menu never is.
const SETTLE_MS: i64 = 800;
/// Off screen this long before it is over, so a full redraw does not end it.
const GONE_MS: i64 = 1_000;
/// Status lines and input boxes a program may keep below its menu.
const MAX_ROWS_BELOW_MENU: usize = 8;
/// Lines of context used as the question when none ends in `?`.
const MAX_CONTEXT_ROWS: usize = 3;
/// Longest question kept, in characters.
const MAX_QUESTION_CHARS: usize = 300;

/// A question on screen and the answers it offers.
#[derive(Debug, Clone)]
pub(crate) struct ScreenQuestion {
    pub(crate) question: String,
    pub(crate) options: Vec<PromptOption>,
    /// The option the selector is on, 0-based (T0.8).
    pub(crate) marked: Option<u8>,
}

/// The same question even when the selector moved: arrowing through a menu
/// never asks it again.
impl PartialEq for ScreenQuestion {
    fn eq(&self, other: &Self) -> bool {
        self.question == other.question && self.options == other.options
    }
}

/// `position` as the marked option, if it is one of the `offered` answers.
fn marked_option(position: Option<usize>, offered: usize) -> Option<u8> {
    position
        .filter(|index| *index < offered)
        .and_then(|index| u8::try_from(index).ok())
}

/// Follows one Session's screen and reports each question once.
#[derive(Debug, Default)]
pub(crate) struct ScreenPrompts {
    pending: Option<(ScreenQuestion, Timestamp)>,
    current: Option<ScreenQuestion>,
    missing_since: Option<Timestamp>,
    /// Input arrived while `current` was asked: once it leaves the screen it was
    /// answered, so it ends at once and the same question again is a new one (X18).
    answered: bool,
    alternate: bool,
}

impl ScreenPrompts {
    /// Whether a question is on screen waiting for a person.
    pub(crate) fn waiting(&self) -> bool {
        self.current.is_some()
    }

    /// Whether a full-screen program has the terminal, so lines mean little.
    pub(crate) fn alternate(&self) -> bool {
        self.alternate
    }

    /// Input was sent to the Session.
    pub(crate) fn input(&mut self) {
        if self.current.is_some() {
            self.answered = true;
        }
    }

    /// The Command ended, so does anything it was asking.
    pub(crate) fn reset(&mut self) {
        *self = ScreenPrompts::default();
    }

    /// Look at the screen again.
    pub(crate) fn observe(
        &mut self,
        at: Timestamp,
        rows: &[String],
        alternate: bool,
    ) -> Vec<EventKind> {
        self.alternate = alternate;
        let Some(found) = find_question(rows) else {
            self.pending = None;
            if self.current.is_some() {
                let since = *self.missing_since.get_or_insert(at);
                if self.answered || at.0.saturating_sub(since.0) >= GONE_MS {
                    self.current = None;
                    self.missing_since = None;
                    self.answered = false;
                }
            }
            return Vec::new();
        };
        self.missing_since = None;
        if self.current.as_ref() == Some(&found) {
            self.pending = None;
            return Vec::new();
        }
        match &self.pending {
            Some((question, since))
                if *question == found && at.0.saturating_sub(since.0) >= SETTLE_MS =>
            {
                self.pending = None;
                self.current = Some(found.clone());
                self.answered = false;
                vec![EventKind::WaitingForInput {
                    prompt: Some(found.question),
                    options: found.options,
                    marked: found.marked,
                }]
            }
            Some((question, _)) if *question == found => Vec::new(),
            _ => {
                self.pending = Some((found, at));
                Vec::new()
            }
        }
    }
}

/// The lowest question on this screen, if any.
pub(crate) fn find_question(rows: &[String]) -> Option<ScreenQuestion> {
    let lines: Vec<String> = rows.iter().map(|row| clean(row)).collect();
    let last = lines.iter().rposition(|line| !line.trim().is_empty())?;
    if let Some(prompt) = letter_choices(&lines[last]) {
        return Some(prompt);
    }
    (0..=last)
        .rev()
        .filter(|index| matches!(option_line(&lines[*index]), Some((1, _, _))))
        .find_map(|index| menu_at(&lines, index, last))
        .or_else(|| marked_menu(&lines, last))
}

/// A marked list of 2–8 short rows under a question, where one row carries a
/// selection marker (`❯ Yes` / `  No`) and a key hint says how to choose.
/// At most [`MAX_PROMPT_OPTIONS`] of the rows are offered as answers.
/// Plain text is never a menu: a marker and a hint are both required.
fn marked_menu(lines: &[String], last: usize) -> Option<ScreenQuestion> {
    // Skip trailing hint/empty lines to find the actual menu rows.
    let mut menu_end = last;
    while menu_end > 0 && (lines[menu_end].trim().is_empty() || is_hint(&lines[menu_end])) {
        menu_end -= 1;
    }
    // Collect consecutive non-empty rows going up from menu_end.
    let mut candidates: Vec<(usize, String, bool)> = Vec::new();
    let mut index = menu_end;
    loop {
        let line = &lines[index];
        if line.trim().is_empty() {
            if candidates.is_empty() {
                if index == 0 {
                    break;
                }
                index -= 1;
                continue;
            }
            break;
        }
        let trimmed = line.trim_start();
        let marked = trimmed.starts_with(MARKERS);
        let label = trimmed.trim_start_matches(MARKERS).trim();
        // Short rows only: a menu option is a few words, not a paragraph.
        // A question line ends the menu.
        if label.is_empty() || label.chars().count() > 80 || label.ends_with('?') {
            break;
        }
        candidates.push((index, label.to_string(), marked));
        if candidates.len() >= 8 {
            break;
        }
        if index == 0 {
            break;
        }
        index -= 1;
    }
    candidates.reverse();
    if candidates.len() < 2 || candidates.len() > 8 {
        return None;
    }
    // Exactly one row must carry a selection marker.
    let marked_count = candidates.iter().filter(|(_, _, m)| *m).count();
    if marked_count != 1 {
        return None;
    }
    // A key hint must exist below the menu.
    let hinted = lines[menu_end + 1..=last].iter().any(|line| is_hint(line));
    if !hinted {
        return None;
    }
    let first = candidates.first()?.0;
    let question = question_above(lines, first, true)?;
    // The protocol carries at most MAX_PROMPT_OPTIONS answers. Like a numbered
    // menu, offer the first ones; the rest are still answered in the terminal.
    let options = candidates
        .iter()
        .take(MAX_PROMPT_OPTIONS)
        .enumerate()
        .map(|(position, (_, label, _))| PromptOption::new(label, &(position + 1).to_string()))
        .collect::<Option<Vec<_>>>()?;
    let marked = marked_option(
        candidates.iter().position(|(_, _, marked)| *marked),
        options.len(),
    );
    Some(ScreenQuestion {
        question,
        options,
        marked,
    })
}

/// The menu whose option 1 is on `first`, if the screen says it is one.
fn menu_at(lines: &[String], first: usize, last: usize) -> Option<ScreenQuestion> {
    let mut options: Vec<String> = Vec::new();
    let mut selected = false;
    let mut marked_at = None;
    let mut column = 0;
    let mut end = first;
    for (index, line) in lines.iter().enumerate().take(last + 1).skip(first) {
        if let Some((number, label, label_column)) = option_line(line) {
            if number as usize != options.len() + 1 || options.len() == MAX_PROMPT_OPTIONS {
                break;
            }
            if line.trim_start().starts_with(MARKERS) {
                selected = true;
                marked_at.get_or_insert(options.len());
            }
            options.push(label.to_string());
            column = label_column;
            end = index;
        } else if !line.trim().is_empty()
            && !is_hint(line)
            && line.chars().take_while(|c| *c == ' ').count() >= column
            && index == end + 1
        {
            // A long option wrapped onto the next row.
            if let Some(option) = options.last_mut() {
                option.push(' ');
                option.push_str(line.trim());
            }
            end = index;
        } else {
            break;
        }
    }
    let below = lines[end + 1..=last]
        .iter()
        .filter(|line| !line.trim().is_empty());
    let hinted = lines[end + 1..=last]
        .iter()
        .take(4)
        .any(|line| is_hint(line));
    if options.len() < 2 || below.count() > MAX_ROWS_BELOW_MENU || !(selected || hinted) {
        return None;
    }
    let question = question_above(lines, first, hinted)?;
    let options = options
        .iter()
        .enumerate()
        .map(|(position, label)| PromptOption::new(label, &(position + 1).to_string()))
        .collect::<Option<Vec<_>>>()?;
    let marked = marked_option(marked_at, options.len());
    Some(ScreenQuestion {
        question,
        options,
        marked,
    })
}

/// The nearest line above ending in `?`. A menu with no question needs key hints
/// below it to be believed, and then the lines that introduce it say what it asks.
fn question_above(lines: &[String], first: usize, hinted: bool) -> Option<String> {
    let mut context: Vec<&str> = Vec::new();
    for line in lines[..first].iter().rev() {
        let text = line.trim();
        if text.is_empty() {
            if context.is_empty() {
                continue;
            }
            break;
        }
        if text.ends_with('?') {
            return Some(bounded(text));
        }
        context.push(text);
        if context.len() == MAX_CONTEXT_ROWS {
            break;
        }
    }
    context.reverse();
    (hinted && !context.is_empty()).then(|| bounded(&context.join(" · ")))
}

fn bounded(text: &str) -> String {
    text.chars().take(MAX_QUESTION_CHARS).collect()
}
