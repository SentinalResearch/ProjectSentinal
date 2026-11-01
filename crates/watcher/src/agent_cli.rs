//! AI coding agents in a Session (Milestone 6.1): Claude Code, Codex, aider,
//! Gemini CLI. Their approval prompts become `WaitingForInput` with the
//! answers they offer, read from the screen:
//!
//! - a question ending in `?` followed by 2–6 numbered choices at the bottom
//!   of the screen (Claude Code's "Do you want to proceed?" dialog, and other
//!   agents' numbered menus), each answered by its digit;
//! - aider's one-line form, `Run shell command? (Y)es/(N)o/(D)on't ask again [Yes]:`,
//!   each answered by its letter and Enter.
//!
//! A dialog is reported once the screen has been quiet for a second, so a
//! half-drawn redraw is never a prompt, and a redraw of the same dialog is
//! not a second one. The wait ends after a dozen lines of other output.
//! Everything here is deterministic; terminal text is never instructions.

use crate::{AdapterState, Observation, ObservationKind, WatcherAdapter};
use protocol::{event::MAX_PROMPT_OPTIONS, Command, EventKind, PromptOption, Timestamp};
use std::collections::VecDeque;

const WINDOW_LINES: usize = 40;
const SETTLE_MS: i64 = 1_000;
const ANSWERED_AFTER_LINES: usize = 12;
const BORDER: &[char] = &[
    '│', '┃', '║', '╭', '╮', '╰', '╯', '─', '━', '═', '┌', '┐', '└', '┘', '|',
];
const MARKERS: &[char] = &['❯', '›', '>', '▶', '●', '○', '*', '◯', '◉'];

/// What the agent adapter currently knows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AgentCliState {
    /// Which agent the Command started, e.g. `"claude"`.
    pub agent: Option<&'static str>,
    /// The approval it is waiting on, if any.
    pub prompt: Option<String>,
    /// Whether it is waiting for a person.
    pub waiting: bool,
    /// Which agent, its activity count, and its usage (6.1).
    pub summary: Option<protocol::coding_agent::CodingAgentSummary>,
}

#[derive(Debug, Clone, PartialEq)]
struct Prompt {
    question: String,
    options: Vec<PromptOption>,
}

/// Watches agent CLIs for approval prompts.
#[derive(Debug, Default)]
pub struct AgentCliAdapter {
    lines: VecDeque<String>,
    candidate: Option<Prompt>,
    last_line_at: Option<Timestamp>,
    emitted: Option<Prompt>,
    lines_since_prompt: usize,
    turn: crate::agent_activity::AgentTurn,
    state: AgentCliState,
}

impl AgentCliAdapter {
    /// An adapter with nothing seen yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether an approval prompt is waiting for a person.
    pub fn waiting(&self) -> bool {
        self.emitted.is_some()
    }

    fn reset(&mut self) {
        let agent = self.state.agent;
        *self = AgentCliAdapter::default();
        self.state.agent = agent;
        self.refresh_summary();
    }

    fn refresh_summary(&mut self) {
        self.state.summary = self
            .state
            .agent
            .and_then(coding_agent)
            .map(|agent| self.turn.summary(agent));
    }

    fn settle(&mut self, at: Timestamp) -> Vec<EventKind> {
        let quiet = self
            .last_line_at
            .is_some_and(|last| at.0.saturating_sub(last.0) >= SETTLE_MS);
        match &self.candidate {
            Some(prompt) if quiet && self.emitted.as_ref() != Some(prompt) => {
                self.emitted = Some(prompt.clone());
                self.lines_since_prompt = 0;
                self.state.prompt = Some(prompt.question.clone());
                self.state.waiting = true;
                vec![EventKind::WaitingForInput {
                    prompt: Some(prompt.question.clone()),
                    options: prompt.options.clone(),
                }]
            }
            _ => Vec::new(),
        }
    }
}

/// The protocol's name for an [`agent_of`] result.
pub fn coding_agent(slug: &str) -> Option<protocol::coding_agent::CodingAgent> {
    use protocol::coding_agent::CodingAgent;
    match slug {
        "claude" => Some(CodingAgent::Claude),
        "codex" => Some(CodingAgent::Codex),
        "aider" => Some(CodingAgent::Aider),
        "gemini" => Some(CodingAgent::Gemini),
        _ => None,
    }
}

/// The agent a Command line starts, skipping `VAR=value` and package runners.
pub fn agent_of(command: &str) -> Option<&'static str> {
    let mut words = command
        .split_whitespace()
        .skip_while(|word| word.contains('=') && !word.starts_with('-'));
    let mut program = words.next()?;
    let runner = matches!(basename(program), "npx" | "bunx" | "pnpx")
        || (matches!(basename(program), "pnpm" | "yarn" | "bun") && words.next() == Some("dlx"));
    if runner {
        program = words.next()?;
    }
    let name = basename(program);
    let name = name.split('@').next().unwrap_or(name);
    match name {
        "claude" | "claude-code" => Some("claude"),
        "codex" => Some("codex"),
        "aider" => Some("aider"),
        "gemini" | "gemini-cli" => Some("gemini"),
        _ => None,
    }
}

fn basename(program: &str) -> &str {
    let trimmed = program.trim_start_matches('@');
    trimmed.rsplit('/').next().unwrap_or(trimmed)
}

impl WatcherAdapter for AgentCliAdapter {
    fn name(&self) -> &'static str {
        "agent-cli"
    }

    fn matches(&self, command: &Command) -> bool {
        agent_of(&command.text).is_some()
    }

    fn observe(&mut self, observation: &Observation) -> Vec<EventKind> {
        match &observation.kind {
            ObservationKind::CommandStarted(command) => {
                self.state.agent = agent_of(&command.text);
                self.reset();
                Vec::new()
            }
            ObservationKind::CommandFinished(_) => {
                // The agent exited: no summary until another one starts.
                self.state.agent = None;
                self.reset();
                Vec::new()
            }
            ObservationKind::OutputLine(line) => {
                let cleaned = clean(line);
                if cleaned.trim().is_empty() {
                    return Vec::new();
                }
                self.last_line_at = Some(observation.at);
                let activity = self.turn.line(observation.at, &cleaned);
                self.refresh_summary();
                self.lines.push_back(cleaned);
                if self.lines.len() > WINDOW_LINES {
                    self.lines.pop_front();
                }
                self.candidate = detect(&self.lines);
                if self.emitted.is_some() {
                    if self.candidate == self.emitted {
                        self.lines_since_prompt = 0;
                    } else {
                        self.lines_since_prompt += 1;
                        if self.lines_since_prompt >= ANSWERED_AFTER_LINES {
                            self.emitted = None;
                            self.state.prompt = None;
                            self.state.waiting = false;
                        }
                    }
                }
                activity
            }
            ObservationKind::Tick | ObservationKind::MetricsSample(_) => {
                let mut events = self.settle(observation.at);
                events.extend(self.turn.tick(observation.at, self.emitted.is_some()));
                self.refresh_summary();
                events
            }
            ObservationKind::ProcessSample(_) => Vec::new(),
        }
    }

    fn state(&self) -> AdapterState {
        AdapterState::AgentCli(self.state.clone())
    }

    fn waiting(&self) -> bool {
        AgentCliAdapter::waiting(self)
    }

    fn owns_prompts(&self) -> bool {
        true
    }
}

/// Remove dialog borders; keep the indentation inside them.
fn clean(line: &str) -> String {
    let line = line.trim_end();
    let line = line.strip_prefix(BORDER).unwrap_or(line);
    let line = line.trim_end_matches(|c: char| BORDER.contains(&c) || c.is_whitespace());
    if line
        .chars()
        .all(|c| BORDER.contains(&c) || c.is_whitespace())
    {
        String::new()
    } else {
        line.to_string()
    }
}

fn detect(lines: &VecDeque<String>) -> Option<Prompt> {
    let last = lines.back()?;
    letter_choices(last).or_else(|| numbered_choices(lines))
}

/// aider: `Question? (Y)es/(N)o/(D)on't ask again [Yes]:`
fn letter_choices(line: &str) -> Option<Prompt> {
    let line = line.trim();
    let body = line.strip_suffix(':')?.trim_end();
    let body = match body.rfind(" [") {
        Some(index) if body.ends_with(']') => body[..index].trim_end(),
        _ => body,
    };
    let start = body.find(" (")? + 1;
    let question = body[..start].trim();
    if !question.ends_with('?') {
        return None;
    }
    let options = body[start..]
        .split('/')
        .map(|choice| {
            let mut chars = choice.chars();
            let (open, letter, close) = (chars.next()?, chars.next()?, chars.next()?);
            if open != '(' || close != ')' || !letter.is_ascii_alphabetic() {
                return None;
            }
            let label = format!("{letter}{}", chars.as_str());
            PromptOption::new(&label, &format!("{}\r", letter.to_ascii_lowercase()))
        })
        .collect::<Option<Vec<_>>>()?;
    (2..=MAX_PROMPT_OPTIONS)
        .contains(&options.len())
        .then(|| Prompt {
            question: question.to_string(),
            options,
        })
}

/// A numbered option line: `❯ 1. Yes`. Returns (number, label, label column in characters).
fn option_line(line: &str) -> Option<(u32, &str, usize)> {
    let mut rest = line.trim_start();
    if let Some(stripped) = rest.strip_prefix(MARKERS) {
        rest = stripped.trim_start();
    }
    let mut chars = rest.chars();
    let digit = chars.next()?.to_digit(10)?;
    if !matches!(chars.next()?, '.' | ')') || chars.next()? != ' ' {
        return None;
    }
    let after = chars.as_str();
    let label = after.trim();
    let column = line.chars().count() - after.chars().count();
    (!label.is_empty()).then_some((digit, label, column))
}

fn leading_spaces(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ').count()
}

fn is_hint(line: &str) -> bool {
    let lower = line.trim().to_ascii_lowercase();
    lower.len() < 100
        && [
            "esc to",
            "to cancel",
            "enter to",
            "tab to",
            "↑/↓",
            "to navigate",
        ]
        .iter()
        .any(|hint| lower.contains(hint))
}

/// Walk up from the bottom: optional hint lines, then numbered options (each
/// may wrap onto indented lines), then the question that asks them.
fn numbered_choices(lines: &VecDeque<String>) -> Option<Prompt> {
    let mut index = lines.len();
    let mut hints = 0;
    while index > 0
        && hints < 2
        && option_line(&lines[index - 1]).is_none()
        && is_hint(&lines[index - 1])
    {
        index -= 1;
        hints += 1;
    }
    let mut options: Vec<(u32, String)> = Vec::new();
    let mut wrapped: Vec<&str> = Vec::new();
    while index > 0 {
        index -= 1;
        let line = lines[index].as_str();
        if let Some((number, label, column)) = option_line(line) {
            if wrapped.len() > 2 || wrapped.iter().any(|text| leading_spaces(text) < column) {
                return None;
            }
            let mut full = label.to_string();
            for text in wrapped.drain(..).rev() {
                full.push(' ');
                full.push_str(text.trim());
            }
            options.push((number, full));
            if options.len() > MAX_PROMPT_OPTIONS {
                return None;
            }
        } else if line.trim().ends_with('?') && wrapped.is_empty() && options.len() >= 2 {
            options.reverse();
            let in_order = options
                .iter()
                .enumerate()
                .all(|(position, (number, _))| *number as usize == position + 1);
            if !in_order {
                return None;
            }
            let options = options
                .iter()
                .map(|(number, label)| PromptOption::new(label, &number.to_string()))
                .collect::<Option<Vec<_>>>()?;
            return Some(Prompt {
                question: line.trim().to_string(),
                options,
            });
        } else if options.is_empty() {
            // The bottom of the screen is not a list of choices.
            return None;
        } else {
            wrapped.push(line);
        }
    }
    None
}
