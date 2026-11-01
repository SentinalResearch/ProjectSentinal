//! What an AI coding agent does, read from its screen (Milestone 6.1).
//!
//! Pure line parsers; [`crate::AgentCliAdapter`] keeps the turn state. The
//! wording comes from the installed tools (see `tests/agent_activity.rs`).
//! Terminal text is data: a tool call's argument is kept, bounded, as the
//! Event's detail and never acted on.

use protocol::{
    coding_agent::{AgentAction, CodingAgent, CodingAgentSummary, MAX_AGENT_DETAIL_CHARS},
    search::clip,
    EventKind, Timestamp,
};
use std::collections::HashSet;

/// Quiet this long after the last working line ends a turn.
const TURN_QUIET_MS: i64 = 3_000;

/// Gemini CLI's finished-call mark and its tool names.
const GEMINI_DONE: char = '\u{2713}';
const GEMINI_TOOLS: &[&str] = &[
    "Shell",
    "ReadFile",
    "ReadFolder",
    "ReadManyFiles",
    "WriteFile",
    "Edit",
    "FindFiles",
    "SearchText",
    "GoogleSearch",
    "WebFetch",
];
/// Test runners, and the tools whose `test` subcommand runs tests.
const TEST_RUNNERS: &[&str] = &[
    "pytest", "vitest", "jest", "rspec", "phpunit", "ctest", "tox",
];
const TEST_SUBCOMMAND_OF: &[&str] = &[
    "cargo", "go", "npm", "pnpm", "yarn", "bun", "deno", "mvn", "gradle", "make", "dotnet", "mix",
    "swift", "just",
];

/// Tokens and cost an agent printed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    /// Tokens in and out, when printed.
    pub tokens: Option<u64>,
    /// Cost in US dollars, when printed.
    pub cost_usd: Option<f64>,
}

/// The status line an agent shows while it works on a turn.
pub fn working_line(line: &str) -> bool {
    line.contains("esc to interrupt") || line.contains("esc to cancel")
}

/// A tool call: Claude Code's `⏺ Name(args)`, Gemini CLI's `✓ Name args`,
/// or aider's `Applied edit to file`.
pub fn tool_line(line: &str) -> Option<(AgentAction, String)> {
    let text = line.trim();
    if let Some(file) = text.strip_prefix("Applied edit to ") {
        return Some((AgentAction::EditedFile, detail(file)));
    }
    if let Some(rest) = text.strip_prefix(['⏺', '●']) {
        let rest = rest.trim_start();
        let open = rest.find('(')?;
        let name = &rest[..open];
        let args = rest[open + 1..].trim_end();
        let args = args.strip_suffix(')').or_else(|| args.strip_suffix('…'))?;
        if !is_tool_name(name) {
            return None;
        }
        return Some((action_for(name, args), detail(args)));
    }
    if let Some(rest) = text.strip_prefix(GEMINI_DONE) {
        let rest = rest.trim_start();
        let (name, args) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if !GEMINI_TOOLS.contains(&name) {
            return None;
        }
        return Some((action_for(name, args.trim()), detail(args)));
    }
    None
}

/// One or two capitalised words of letters: `Bash`, `Web Search`.
fn is_tool_name(name: &str) -> bool {
    let words: Vec<&str> = name.split(' ').collect();
    (1..=2).contains(&words.len())
        && words.iter().all(|word| {
            word.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && word.chars().all(|c| c.is_ascii_alphabetic())
        })
}

fn action_for(name: &str, args: &str) -> AgentAction {
    match name {
        "Bash" | "PowerShell" | "Shell" if is_test_command(args) => AgentAction::RanTests,
        "Bash" | "PowerShell" | "Shell" => AgentAction::RanCommand,
        "Edit" | "Update" | "Write" | "MultiEdit" | "NotebookEdit" | "Edit Notebook"
        | "WriteFile" => AgentAction::EditedFile,
        "Read" | "ReadFile" | "ReadFolder" | "ReadManyFiles" | "LS" => AgentAction::ReadFile,
        "Search" | "Grep" | "Glob" | "Web Search" | "WebSearch" | "WebFetch" | "Fetch"
        | "FindFiles" | "SearchText" | "GoogleSearch" => AgentAction::Searched,
        _ => AgentAction::UsedTool,
    }
}

fn is_test_command(command: &str) -> bool {
    let words: Vec<&str> = command.split_whitespace().collect();
    words.iter().enumerate().any(|(index, word)| {
        let program = word.rsplit('/').next().unwrap_or(word);
        TEST_RUNNERS.contains(&program)
            || (*word == "test"
                && index > 0
                && TEST_SUBCOMMAND_OF.contains(&words[index - 1].rsplit('/').next().unwrap_or("")))
    })
}

fn detail(text: &str) -> String {
    clip(text, MAX_AGENT_DETAIL_CHARS)
}

/// An API failure the agent printed, from `API Error:` on.
pub fn api_error(line: &str) -> Option<String> {
    line.find("API Error:")
        .map(|at| clip(&line[at..], MAX_AGENT_DETAIL_CHARS))
}

/// Usage an agent printed: Claude Code's `/cost` lines (`Total cost:`,
/// `Usage: N input, M output, …`) or aider's `Tokens: … Cost: … session.`
pub fn usage_line(line: &str) -> Option<Usage> {
    let text = line.trim();
    if let Some(rest) = text.strip_prefix("Total cost:") {
        let cost = dollars(rest.trim())?;
        return Some(Usage {
            tokens: None,
            cost_usd: Some(cost),
        });
    }
    if let Some(rest) = text.strip_prefix("Usage:") {
        let input = count_before(rest, "input")?;
        let output = count_before(rest, "output")?;
        return Some(Usage {
            tokens: Some(input + output),
            cost_usd: None,
        });
    }
    if let Some(rest) = text.strip_prefix("Tokens:") {
        let sent = count_before(rest, "sent")?;
        let received = count_before(rest, "received")?;
        let cost = rest
            .split_once("Cost:")
            .and_then(|(_, costs)| costs.split(',').find(|part| part.contains("session")))
            .and_then(|part| dollars(part.trim()));
        return Some(Usage {
            tokens: Some(sent + received),
            cost_usd: cost,
        });
    }
    None
}

/// `$1.25` (followed by anything) as dollars.
fn dollars(text: &str) -> Option<f64> {
    let number: String = text
        .strip_prefix('$')?
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    number.parse().ok().filter(|value: &f64| value.is_finite())
}

/// The count just before `label`, e.g. `12.5k input` → 12 500.
fn count_before(text: &str, label: &str) -> Option<u64> {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|word| !word.is_empty())
        .collect();
    let index = words
        .iter()
        .position(|word| word.trim_end_matches(['.', ',']) == label)?;
    count(words.get(index.checked_sub(1)?)?)
}

/// `312`, `2.4k`, `1m` as a whole number.
fn count(word: &str) -> Option<u64> {
    let lower = word.to_ascii_lowercase();
    let (number, scale) = match lower.strip_suffix('k') {
        Some(number) => (number, 1_000.0),
        None => match lower.strip_suffix('m') {
            Some(number) => (number, 1_000_000.0),
            None => (lower.as_str(), 1.0),
        },
    };
    let value: f64 = number.parse().ok()?;
    (value.is_finite() && value >= 0.0).then(|| (value * scale).round() as u64)
}

/// One agent's turns: activity Events, the end of each turn, and usage.
#[derive(Debug, Default)]
pub struct AgentTurn {
    working: bool,
    last_working_at: Option<Timestamp>,
    turn_tools: u32,
    total_tools: u32,
    // A redraw repeats the live region; each call and error counts once a turn.
    seen_tools: HashSet<(AgentAction, String)>,
    seen_errors: HashSet<String>,
    tokens: Option<u64>,
    cost_usd: Option<f64>,
}

impl AgentTurn {
    /// Read one output line.
    pub fn line(&mut self, at: Timestamp, line: &str) -> Vec<EventKind> {
        if working_line(line) {
            self.working = true;
            self.last_working_at = Some(at);
        }
        if let Some(usage) = usage_line(line) {
            self.tokens = usage.tokens.or(self.tokens);
            self.cost_usd = usage.cost_usd.or(self.cost_usd);
        }
        if let Some(message) = api_error(line) {
            return if self.seen_errors.insert(message.clone()) {
                vec![EventKind::ErrorDetected { message }]
            } else {
                Vec::new()
            };
        }
        match tool_line(line) {
            Some((action, detail)) if self.seen_tools.insert((action, detail.clone())) => {
                self.turn_tools += 1;
                self.total_tools += 1;
                vec![EventKind::AgentActivity { action, detail }]
            }
            _ => Vec::new(),
        }
    }

    /// Time passed. A turn ends once the working line has been gone for a
    /// while, unless the agent is waiting on an approval prompt.
    pub fn tick(&mut self, at: Timestamp, waiting: bool) -> Vec<EventKind> {
        let quiet = self
            .last_working_at
            .is_some_and(|last| at.0.saturating_sub(last.0) >= TURN_QUIET_MS);
        if !self.working || !quiet || waiting {
            return Vec::new();
        }
        let tool_uses = self.turn_tools;
        self.working = false;
        self.turn_tools = 0;
        self.seen_tools.clear();
        self.seen_errors.clear();
        vec![EventKind::AgentTurnFinished { tool_uses }]
    }

    /// The summary for `agent`.
    pub fn summary(&self, agent: CodingAgent) -> CodingAgentSummary {
        CodingAgentSummary {
            agent,
            working: self.working,
            tool_uses: self.total_tools,
            tokens: self.tokens,
            cost_usd: self.cost_usd,
        }
    }
}
