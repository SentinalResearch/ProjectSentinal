//! pytest collection, progress, failure, and final-summary recognition.

use crate::{AdapterState, Observation, ObservationKind, WatcherAdapter};
use protocol::{Command, EventKind};

/// Current structured pytest state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PytestState {
    /// Number of collected tests.
    pub collected: Option<u64>,
    /// Latest completed-test estimate.
    pub completed: Option<u64>,
    /// Final passed count.
    pub passed: Option<u64>,
    /// Final failed count.
    pub failed: Option<u64>,
    /// Failing test node IDs.
    pub failing_tests: Vec<String>,
}

/// Adapter for pytest Commands.
pub struct PytestAdapter {
    state: PytestState,
}

impl PytestAdapter {
    pub(crate) fn new() -> Self {
        PytestAdapter {
            state: PytestState {
                collected: None,
                completed: None,
                passed: None,
                failed: None,
                failing_tests: Vec::new(),
            },
        }
    }
}

impl WatcherAdapter for PytestAdapter {
    fn name(&self) -> &'static str {
        "pytest"
    }

    fn matches(&self, command: &Command) -> bool {
        command.text.to_ascii_lowercase().contains("pytest")
    }

    fn observe(&mut self, observation: &Observation) -> Vec<EventKind> {
        let ObservationKind::OutputLine(line) = &observation.kind else {
            return Vec::new();
        };
        let lower = line.to_ascii_lowercase();
        if let Some(index) = lower.find("collected ") {
            self.state.collected = unsigned_prefix(&lower[index + 10..]);
        }
        let mut events = Vec::new();
        if let (Some(percent), Some(total)) = (percent(line), self.state.collected) {
            let completed = total.saturating_mul(percent).saturating_add(99) / 100;
            self.state.completed = Some(completed.min(total));
            events.push(EventKind::ProgressUpdated {
                current: completed.min(total),
                total: Some(total),
                unit: "test".into(),
            });
        }
        if let Some(name) = line
            .strip_prefix("FAILED ")
            .and_then(|rest| rest.split_whitespace().next())
        {
            self.state.failing_tests.push(name.to_string());
            events.push(EventKind::ErrorDetected {
                message: format!("pytest failed {name}"),
            });
        }
        if lower.contains(" passed") || lower.contains(" failed") {
            let passed = count_before(&lower, " passed");
            let failed = count_before(&lower, " failed");
            if passed.is_some() || failed.is_some() {
                self.state.passed = passed.or(Some(0));
                self.state.failed = failed.or(Some(0));
                let complete = passed.unwrap_or(0).saturating_add(failed.unwrap_or(0));
                let total = self.state.collected.or(Some(complete));
                events.push(EventKind::ProgressUpdated {
                    current: complete,
                    total,
                    unit: "test".into(),
                });
                if failed.unwrap_or(0) > 0 {
                    events.push(EventKind::ErrorDetected {
                        message: format!(
                            "pytest summary: {} passed, {} failed",
                            passed.unwrap_or(0),
                            failed.unwrap_or(0)
                        ),
                    });
                }
            }
        }
        events
    }

    fn state(&self) -> AdapterState {
        AdapterState::Pytest(self.state.clone())
    }
}

fn unsigned_prefix(value: &str) -> Option<u64> {
    let digits: String = value.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn count_before(line: &str, suffix: &str) -> Option<u64> {
    let index = line.find(suffix)?;
    let before = line[..index].trim_end();
    let start = before
        .rfind(|character: char| !character.is_ascii_digit())
        .map_or(0, |position| position + 1);
    before[start..].parse().ok()
}

fn percent(line: &str) -> Option<u64> {
    let index = line.find('%')?;
    let before = &line[..index];
    let start = before
        .rfind(|character: char| !character.is_ascii_digit())
        .map_or(0, |position| position + 1);
    before[start..].parse().ok()
}
