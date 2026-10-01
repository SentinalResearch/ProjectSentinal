//! User-defined watcher adapters loaded from bounded TOML files.

use crate::{AdapterState, Observation, ObservationKind, WatcherAdapter};
use protocol::{Command, EventKind};
use regex::Regex;
use serde::Deserialize;
use thiserror::Error;

/// Declarative adapter load failure.
#[derive(Debug, Error)]
pub enum DeclarativeError {
    /// The TOML document was invalid.
    #[error("invalid declarative adapter '{file}': {source}")]
    InvalidToml {
        /// Adapter file name.
        file: String,
        /// Parser failure.
        source: toml::de::Error,
    },
    /// The command-matching expression was invalid.
    #[error("invalid match_command regex in declarative adapter '{file}': {source}")]
    InvalidMatchRegex {
        /// Adapter file name.
        file: String,
        /// Regex failure.
        source: regex::Error,
    },
    /// A rule expression was invalid.
    #[error("invalid regex in declarative adapter '{file}', rule {rule}: {source}")]
    InvalidRuleRegex {
        /// Adapter file name.
        file: String,
        /// One-based rule number.
        rule: usize,
        /// Regex failure.
        source: regex::Error,
    },
    /// A rule requests an unsupported Event kind.
    #[error("unsupported emit '{emit}' in declarative adapter '{file}', rule {rule}")]
    UnsupportedEmit {
        /// Adapter file name.
        file: String,
        /// One-based rule number.
        rule: usize,
        /// Requested kind.
        emit: String,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    name: String,
    match_command: String,
    rule: Vec<RuleConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct RuleConfig {
    regex: String,
    emit: String,
    unit: Option<String>,
}

#[derive(Debug, Clone)]
struct CompiledRule {
    regex: Regex,
    emit: DeclarativeEmit,
    unit: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum DeclarativeEmit {
    Progress,
    Warning,
    Error,
    Checkpoint,
    Server,
}

/// A validated TOML-defined watcher adapter.
#[derive(Debug, Clone)]
pub struct DeclarativeAdapter {
    name: String,
    command: Regex,
    rules: Vec<CompiledRule>,
    matches: u64,
}

/// Current structured declarative-adapter state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclarativeState {
    /// Configured adapter name.
    pub name: String,
    /// Number of rules that emitted an Event.
    pub matches: u64,
}

impl DeclarativeAdapter {
    /// Parse and compile one declarative adapter.
    pub fn parse(file: impl Into<String>, text: &str) -> Result<Self, DeclarativeError> {
        let file = file.into();
        let config: Config =
            toml::from_str(text).map_err(|source| DeclarativeError::InvalidToml {
                file: file.clone(),
                source,
            })?;
        let command = Regex::new(&config.match_command).map_err(|source| {
            DeclarativeError::InvalidMatchRegex {
                file: file.clone(),
                source,
            }
        })?;
        let mut rules = Vec::with_capacity(config.rule.len());
        for (index, rule) in config.rule.into_iter().enumerate() {
            let number = index + 1;
            let regex =
                Regex::new(&rule.regex).map_err(|source| DeclarativeError::InvalidRuleRegex {
                    file: file.clone(),
                    rule: number,
                    source,
                })?;
            let emit = match rule.emit.as_str() {
                "PROGRESS_UPDATED" => DeclarativeEmit::Progress,
                "WARNING_DETECTED" => DeclarativeEmit::Warning,
                "ERROR_DETECTED" => DeclarativeEmit::Error,
                "CHECKPOINT_CREATED" => DeclarativeEmit::Checkpoint,
                "SERVER_STARTED" => DeclarativeEmit::Server,
                _ => {
                    return Err(DeclarativeError::UnsupportedEmit {
                        file,
                        rule: number,
                        emit: rule.emit,
                    });
                }
            };
            rules.push(CompiledRule {
                regex,
                emit,
                unit: rule.unit,
            });
        }
        Ok(DeclarativeAdapter {
            name: config.name,
            command,
            rules,
            matches: 0,
        })
    }
}

impl WatcherAdapter for DeclarativeAdapter {
    fn name(&self) -> &'static str {
        "declarative"
    }

    fn matches(&self, command: &Command) -> bool {
        self.command.is_match(&command.text)
    }

    fn observe(&mut self, observation: &Observation) -> Vec<EventKind> {
        let ObservationKind::OutputLine(line) = &observation.kind else {
            return Vec::new();
        };
        let mut events = Vec::new();
        for rule in &self.rules {
            let Some(captures) = rule.regex.captures(line) else {
                continue;
            };
            let event = match rule.emit {
                DeclarativeEmit::Progress => progress_event(&captures, rule.unit.as_deref()),
                DeclarativeEmit::Warning => Some(EventKind::WarningDetected {
                    message: capture(&captures, "message").unwrap_or(line).to_string(),
                }),
                DeclarativeEmit::Error => Some(EventKind::ErrorDetected {
                    message: capture(&captures, "message").unwrap_or(line).to_string(),
                }),
                DeclarativeEmit::Checkpoint => capture(&captures, "path")
                    .map(|path| EventKind::CheckpointCreated { path: path.into() }),
                DeclarativeEmit::Server => Some(EventKind::ServerStarted {
                    port: capture(&captures, "port").and_then(|value| value.parse().ok()),
                    url: capture(&captures, "url").map(ToOwned::to_owned),
                }),
            };
            if let Some(event) = event {
                self.matches = self.matches.saturating_add(1);
                events.push(event);
            }
        }
        events
    }

    fn state(&self) -> AdapterState {
        AdapterState::Declarative(DeclarativeState {
            name: self.name.clone(),
            matches: self.matches,
        })
    }
}

fn capture<'a>(captures: &'a regex::Captures<'_>, name: &str) -> Option<&'a str> {
    captures.name(name).map(|value| value.as_str())
}

fn progress_event(captures: &regex::Captures<'_>, unit: Option<&str>) -> Option<EventKind> {
    let current = capture(captures, "current")?.parse().ok()?;
    let total = capture(captures, "total").and_then(|value| value.parse().ok());
    Some(EventKind::ProgressUpdated {
        current,
        total,
        unit: unit.unwrap_or("step").to_string(),
    })
}
