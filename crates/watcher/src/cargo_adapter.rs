//! Cargo build and test output recognition.

use crate::{AdapterState, Observation, ObservationKind, WatcherAdapter};
use protocol::{Command, EventKind};

/// Current structured Cargo state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoState {
    /// Crate names observed in `Compiling` lines.
    pub compiled: Vec<String>,
    /// Final passed-test count.
    pub tests_passed: Option<u64>,
    /// Final failed-test count.
    pub tests_failed: Option<u64>,
}

/// Adapter for Cargo Commands.
pub struct CargoAdapter {
    state: CargoState,
}

impl CargoAdapter {
    pub(crate) fn new() -> Self {
        CargoAdapter {
            state: CargoState {
                compiled: Vec::new(),
                tests_passed: None,
                tests_failed: None,
            },
        }
    }
}

impl WatcherAdapter for CargoAdapter {
    fn name(&self) -> &'static str {
        "cargo"
    }

    fn matches(&self, command: &Command) -> bool {
        command
            .text
            .split_whitespace()
            .next()
            .is_some_and(|program| program.ends_with("cargo"))
    }

    fn observe(&mut self, observation: &Observation) -> Vec<EventKind> {
        let ObservationKind::OutputLine(line) = &observation.kind else {
            return Vec::new();
        };
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("Compiling ") {
            if let Some(name) = rest.split_whitespace().next() {
                self.state.compiled.push(name.to_string());
                return vec![EventKind::ProgressUpdated {
                    current: self.state.compiled.len() as u64,
                    total: None,
                    unit: "crate".into(),
                }];
            }
        }
        if trimmed.starts_with("error[E") || trimmed.starts_with("error:") {
            return vec![EventKind::ErrorDetected {
                message: trimmed.to_string(),
            }];
        }
        if trimmed.starts_with("test result:") {
            let passed = count_before(trimmed, " passed").unwrap_or(0);
            let failed = count_before(trimmed, " failed").unwrap_or(0);
            self.state.tests_passed = Some(passed);
            self.state.tests_failed = Some(failed);
            let mut events = vec![EventKind::ProgressUpdated {
                current: passed.saturating_add(failed),
                total: Some(passed.saturating_add(failed)),
                unit: "test".into(),
            }];
            if failed > 0 {
                events.push(EventKind::ErrorDetected {
                    message: format!("cargo test summary: {passed} passed, {failed} failed"),
                });
            }
            return events;
        }
        Vec::new()
    }

    fn state(&self) -> AdapterState {
        AdapterState::Cargo(self.state.clone())
    }
}

fn count_before(line: &str, suffix: &str) -> Option<u64> {
    let index = line.find(suffix)?;
    let before = line[..index].trim_end();
    let start = before
        .rfind(|character: char| !character.is_ascii_digit())
        .map_or(0, |position| position + 1);
    before[start..].parse().ok()
}
