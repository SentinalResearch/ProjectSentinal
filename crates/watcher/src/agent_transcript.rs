//! Claude Code's own conversation file, read as chat entries and progress (AC1).
//!
//! Design: `docs/AGENT_CHAT_DESIGN.md`. The file is JSON Lines that Claude Code
//! writes as it works; its format is not documented as stable, so only a few
//! fields are read and every other field or record type is skipped:
//!
//! - `type: "user"` with string `content`: the owner's message, which starts
//!   a turn. Claude Code's own `<command-…>` notes are not messages.
//! - `type: "user"` with `tool_result` blocks: results, with `is_error`.
//! - `type: "assistant"` with `text` and `tool_use` blocks; `stop_reason:
//!   "end_turn"` ends the turn. `thinking` is dropped (decision 2).
//! - `TaskCreate` / `TaskUpdate` (and the older `TodoWrite`) tool calls are
//!   the agent's plan: they give "step N of M" without reading the screen.
//!
//! Account and organisation IDs, attachments and request records are never
//! read. Text goes through [`crate::redact::redact_secrets`] (decision 4).
//! This module does no I/O; the daemon follows the file and feeds it lines.

use crate::redact::redact_secrets;
use protocol::agent_chat::{
    clip, MAX_CHAT_PREVIEW_CHARS, MAX_CHAT_SUMMARY_CHARS, MAX_CHAT_TEXT_CHARS,
};
use protocol::{ChatEntry, ChatItem, Timestamp};
use serde_json::Value;
use std::collections::HashMap;

/// Longest line read; a longer one (a huge tool result) is skipped.
pub const MAX_TRANSCRIPT_LINE_BYTES: usize = 1 << 20;
/// Most characters in the task shown for the turn.
const MAX_TASK_CHARS: usize = 80;
/// Most characters in the activity line.
const MAX_DETAIL_CHARS: usize = 200;

/// What the agent is doing, from its own file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptProgress {
    /// A turn is running.
    pub working: bool,
    /// The plan item in progress, else what the owner last asked.
    pub task: Option<String>,
    /// The plan's current step, when the agent keeps a plan.
    pub step: Option<u32>,
    /// Steps in the plan.
    pub steps: Option<u32>,
    /// What it is doing right now, e.g. `Running cargo test`.
    pub detail: Option<String>,
}

/// What one line added.
#[derive(Debug, Default)]
pub struct TranscriptUpdate {
    /// New chat entries, oldest first.
    pub entries: Vec<ChatEntry>,
    /// The new progress, when it changed.
    pub progress: Option<TranscriptProgress>,
}

#[derive(Debug, Clone)]
struct PlanItem {
    id: String,
    subject: String,
    status: String,
}

/// Follows one conversation file, line by line.
#[derive(Debug, Default)]
pub struct TranscriptReader {
    plan: Vec<PlanItem>,
    /// `TaskCreate` calls waiting for their "Task #N created" result.
    creating: HashMap<String, String>,
    working: bool,
    asked: Option<String>,
    detail: Option<String>,
    turn_tools: u32,
    skipped: u32,
    read: u32,
}

impl TranscriptReader {
    /// Lines that could not be read, of all lines seen.
    pub fn unreadable(&self) -> (u32, u32) {
        (self.skipped, self.read)
    }

    /// Read one line of the file.
    pub fn line(&mut self, line: &str) -> TranscriptUpdate {
        self.read = self.read.saturating_add(1);
        if line.len() > MAX_TRANSCRIPT_LINE_BYTES {
            self.skipped = self.skipped.saturating_add(1);
            return TranscriptUpdate::default();
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            self.skipped = self.skipped.saturating_add(1);
            return TranscriptUpdate::default();
        };
        let kind = record.get("type").and_then(Value::as_str);
        if !matches!(kind, Some("user" | "assistant"))
            || record.get("isMeta").and_then(Value::as_bool) == Some(true)
            // Sub-agent work: its own conversation, not this one.
            || record.get("isSidechain").and_then(Value::as_bool) == Some(true)
        {
            return TranscriptUpdate::default();
        }
        let at = record
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339)
            .unwrap_or(Timestamp(0));
        let Some(message) = record.get("message") else {
            return TranscriptUpdate::default();
        };
        let before = self.progress();
        let mut entries = Vec::new();
        if kind == Some("user") {
            self.user(at, message.get("content"), &mut entries);
        } else {
            self.assistant(at, message, &mut entries);
        }
        let after = self.progress();
        TranscriptUpdate {
            entries,
            progress: (after != before).then_some(after),
        }
    }

    /// The progress as it stands.
    pub fn progress(&self) -> TranscriptProgress {
        let live: Vec<&PlanItem> = self.plan.iter().filter(|i| i.status != "deleted").collect();
        let done = live.iter().filter(|i| i.status == "completed").count();
        let current = live.iter().find(|i| i.status == "in_progress");
        let steps = u32::try_from(live.len()).ok().filter(|n| *n > 0);
        let step = steps.map(|steps| {
            let step = done + usize::from(current.is_some());
            u32::try_from(step).unwrap_or(steps).clamp(1, steps)
        });
        TranscriptProgress {
            working: self.working,
            task: current
                .map(|item| clip(&item.subject, MAX_TASK_CHARS))
                .or_else(|| self.asked.clone()),
            step,
            steps,
            detail: self.detail.clone(),
        }
    }

    fn user(&mut self, at: Timestamp, content: Option<&Value>, entries: &mut Vec<ChatEntry>) {
        match content {
            Some(Value::String(text)) => self.asked_by_owner(at, text, entries),
            Some(Value::Array(blocks)) => {
                for block in blocks {
                    match block.get("type").and_then(Value::as_str) {
                        Some("tool_result") => self.tool_result(at, block, entries),
                        Some("text") => {
                            if let Some(text) = block.get("text").and_then(Value::as_str) {
                                self.asked_by_owner(at, text, entries);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn asked_by_owner(&mut self, at: Timestamp, text: &str, entries: &mut Vec<ChatEntry>) {
        let text = text.trim();
        // Claude Code's notes about slash commands and their output.
        if text.is_empty() || text.starts_with('<') || text.starts_with("Caveat:") {
            return;
        }
        let text = redact_secrets(text);
        self.working = true;
        self.turn_tools = 0;
        self.detail = None;
        self.asked = Some(clip(first_line(&text), MAX_TASK_CHARS));
        entries.push(ChatEntry {
            at,
            item: ChatItem::User {
                text: clip(&text, MAX_CHAT_TEXT_CHARS),
            },
        });
    }

    fn tool_result(&mut self, at: Timestamp, block: &Value, entries: &mut Vec<ChatEntry>) {
        let Some(id) = block.get("tool_use_id").and_then(Value::as_str) else {
            return;
        };
        let text = result_text(block.get("content"));
        if let Some(subject) = self.creating.remove(id) {
            if let Some(number) = created_task_number(&text) {
                self.plan.push(PlanItem {
                    id: number,
                    subject,
                    status: "pending".into(),
                });
            }
        }
        let ok = block.get("is_error").and_then(Value::as_bool) != Some(true);
        entries.push(ChatEntry {
            at,
            item: ChatItem::ToolResult {
                id: id.to_string(),
                ok,
                preview: clip(&redact_secrets(&text), MAX_CHAT_PREVIEW_CHARS),
            },
        });
    }

    fn assistant(&mut self, at: Timestamp, message: &Value, entries: &mut Vec<ChatEntry>) {
        if let Some(blocks) = message.get("content").and_then(Value::as_array) {
            for block in blocks {
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                        if !text.trim().is_empty() {
                            entries.push(ChatEntry {
                                at,
                                item: ChatItem::Agent {
                                    text: clip(&redact_secrets(text), MAX_CHAT_TEXT_CHARS),
                                },
                            });
                        }
                    }
                    Some("tool_use") => self.tool_use(at, block, entries),
                    _ => {}
                }
            }
        }
        if message.get("stop_reason").and_then(Value::as_str) == Some("end_turn") && self.working {
            self.working = false;
            self.detail = None;
            entries.push(ChatEntry {
                at,
                item: ChatItem::TurnFinished {
                    tool_uses: self.turn_tools,
                },
            });
            self.turn_tools = 0;
        }
    }

    fn tool_use(&mut self, at: Timestamp, block: &Value, entries: &mut Vec<ChatEntry>) {
        let id = block
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let tool = block.get("name").and_then(Value::as_str).unwrap_or("tool");
        let input = block.get("input").cloned().unwrap_or(Value::Null);
        self.plan_call(&id, tool, &input);
        let summary = clip(
            &redact_secrets(&summary(tool, &input)),
            MAX_CHAT_SUMMARY_CHARS,
        );
        if !is_plan_tool(tool) {
            self.turn_tools = self.turn_tools.saturating_add(1);
            self.detail = Some(clip(&activity(tool, &summary), MAX_DETAIL_CHARS));
        }
        entries.push(ChatEntry {
            at,
            item: ChatItem::Tool {
                id,
                tool: clip(tool, 40),
                summary,
            },
        });
    }

    /// The agent's plan: `TaskCreate`, `TaskUpdate`, or a whole `TodoWrite` list.
    fn plan_call(&mut self, id: &str, tool: &str, input: &Value) {
        let text = |key: &str| input.get(key).and_then(Value::as_str).map(str::to_string);
        match tool {
            "TaskCreate" => {
                if let Some(subject) = text("subject") {
                    self.creating.insert(id.to_string(), subject);
                }
            }
            "TaskUpdate" => {
                if let (Some(task), Some(status)) = (text("taskId"), text("status")) {
                    if let Some(item) = self.plan.iter_mut().find(|item| item.id == task) {
                        item.status = status;
                    }
                }
            }
            "TodoWrite" => {
                if let Some(todos) = input.get("todos").and_then(Value::as_array) {
                    self.plan = todos
                        .iter()
                        .enumerate()
                        .filter_map(|(index, todo)| {
                            Some(PlanItem {
                                id: (index + 1).to_string(),
                                subject: todo.get("content")?.as_str()?.to_string(),
                                status: todo.get("status")?.as_str()?.to_string(),
                            })
                        })
                        .collect();
                }
            }
            _ => {}
        }
    }
}

/// Plan bookkeeping and loading tools: not work, so not counted or shown as activity.
fn is_plan_tool(tool: &str) -> bool {
    matches!(
        tool,
        "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet" | "TodoWrite" | "ToolSearch"
    )
}

/// One line saying what the call does.
fn summary(tool: &str, input: &Value) -> String {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let found = match tool {
        "Bash" | "PowerShell" => text("command"),
        "Read" | "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
            text("file_path").or_else(|| text("notebook_path"))
        }
        "Grep" | "Glob" => text("pattern"),
        "WebFetch" => text("url"),
        "WebSearch" => text("query"),
        "Task" | "Agent" => text("description"),
        "TaskCreate" => text("subject"),
        _ => None,
    };
    if let Some(found) = found {
        return first_line(found).to_string();
    }
    match tool {
        "TaskUpdate" => format!(
            "task {} → {}",
            text("taskId").unwrap_or("?"),
            text("status").unwrap_or("updated")
        ),
        "TodoWrite" => {
            let n = input
                .get("todos")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            format!("{n} plan items")
        }
        _ => input
            .as_object()
            .and_then(|fields| fields.values().find_map(Value::as_str))
            .map(|value| first_line(value).to_string())
            .unwrap_or_default(),
    }
}

/// `Running cargo test`, `Editing src/api.rs`.
fn activity(tool: &str, summary: &str) -> String {
    let verb = match tool {
        "Bash" | "PowerShell" => "Running",
        "Edit" | "MultiEdit" | "NotebookEdit" => "Editing",
        "Write" => "Writing",
        "Read" => "Reading",
        "Grep" | "Glob" | "WebSearch" => "Searching",
        "WebFetch" => "Fetching",
        "Task" | "Agent" => "Delegating",
        _ => return format!("Using {tool}"),
    };
    if summary.is_empty() {
        verb.to_string()
    } else {
        format!("{verb} {summary}")
    }
}

fn first_line(text: &str) -> &str {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
}

fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// `Task #3 created successfully: …` → `3`.
fn created_task_number(text: &str) -> Option<String> {
    let rest = text.split("Task #").nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

/// `2026-11-19T10:26:15.123Z` (UTC, as Claude Code writes it) → milliseconds.
pub fn parse_rfc3339(text: &str) -> Option<Timestamp> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let mut date = date.split('-').map(str::parse::<i64>);
    let (year, month, day) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
    let (clock, fraction) = time.split_once('.').unwrap_or((time, "0"));
    let mut clock = clock.split(':').map(str::parse::<i64>);
    let (hour, minute, second) = (
        clock.next()?.ok()?,
        clock.next()?.ok()?,
        clock.next()?.ok()?,
    );
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let millis: i64 = format!("{:0<3}", fraction.get(..3.min(fraction.len()))?)
        .parse()
        .ok()?;
    // Days from civil (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(Timestamp(
        (((days * 24 + hour) * 60 + minute) * 60 + second) * 1_000 + millis,
    ))
}
