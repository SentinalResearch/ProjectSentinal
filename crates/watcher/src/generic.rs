//! Generic prompt, quiet-resource, warning, and error detection.

use crate::{AdapterState, Observation, ObservationKind, WatcherAdapter};
use protocol::{Command, DurationMs, EventKind, Timestamp};

const WAITING_QUIET_MS: i64 = 2_000;

/// Current generic watcher state.
#[derive(Debug, Clone, PartialEq)]
pub struct GenericState {
    /// Last bounded output line.
    pub last_line: Option<String>,
    /// Whether a waiting prompt has been emitted.
    pub waiting: bool,
    /// Whether the idle-resource threshold is crossed.
    pub stalled: bool,
}

/// Always-on deterministic watcher for common terminal behavior.
pub struct GenericAdapter {
    stall_after: DurationMs,
    last_output_at: Option<Timestamp>,
    prompt: Option<(String, Timestamp)>,
    last_metrics: Option<(f32, Option<f32>)>,
    state: GenericState,
}

impl GenericAdapter {
    /// Create a Generic adapter with an injected stall duration.
    pub fn new(stall_after: DurationMs) -> Self {
        GenericAdapter {
            stall_after,
            last_output_at: None,
            prompt: None,
            last_metrics: None,
            state: GenericState {
                last_line: None,
                waiting: false,
                stalled: false,
            },
        }
    }

    fn timers(&mut self, at: Timestamp) -> Vec<EventKind> {
        let mut events = Vec::new();
        if !self.state.waiting {
            if let Some((prompt, observed_at)) = &self.prompt {
                if at.0.saturating_sub(observed_at.0) >= WAITING_QUIET_MS {
                    self.state.waiting = true;
                    events.push(EventKind::WaitingForInput {
                        prompt: Some(prompt.clone()),
                    });
                }
            }
        }
        if !self.state.stalled && self.prompt.is_none() {
            if let (Some(last), Some((cpu, gpu))) = (self.last_output_at, self.last_metrics) {
                let quiet = at.0.saturating_sub(last.0);
                let quiet = u64::try_from(quiet).unwrap_or(0);
                if quiet >= self.stall_after.0 && cpu < 5.0 && gpu.unwrap_or(0.0) < 5.0 {
                    self.state.stalled = true;
                    events.push(EventKind::PossiblyStalled {
                        silent_for_ms: DurationMs(quiet),
                    });
                }
            }
        }
        events
    }
}

impl WatcherAdapter for GenericAdapter {
    fn name(&self) -> &'static str {
        "generic"
    }

    fn matches(&self, _command: &Command) -> bool {
        true
    }

    fn observe(&mut self, observation: &Observation) -> Vec<EventKind> {
        match &observation.kind {
            ObservationKind::OutputLine(line) => {
                self.last_output_at = Some(observation.at);
                self.state.last_line = Some(line.clone());
                self.state.waiting = false;
                self.state.stalled = false;
                self.prompt = prompt_line(line).map(|value| (value, observation.at));
                classify_line(line).into_iter().collect()
            }
            ObservationKind::MetricsSample(metrics) => {
                if self.last_output_at.is_none() {
                    self.last_output_at = Some(observation.at);
                }
                self.last_metrics = Some((metrics.cpu_percent, metrics.gpu_percent));
                self.timers(observation.at)
            }
            ObservationKind::Tick => self.timers(observation.at),
            ObservationKind::CommandStarted(_) => {
                self.last_output_at = Some(observation.at);
                self.prompt = None;
                self.state.waiting = false;
                self.state.stalled = false;
                Vec::new()
            }
            ObservationKind::CommandFinished(_) => {
                self.prompt = None;
                self.state.waiting = false;
                self.state.stalled = false;
                Vec::new()
            }
            ObservationKind::ProcessSample(_) => Vec::new(),
        }
    }

    fn state(&self) -> AdapterState {
        AdapterState::Generic(self.state.clone())
    }
}

fn prompt_line(line: &str) -> Option<String> {
    let trimmed = line.trim_end_matches(['\r', '\n']);
    let lower = trimmed.to_ascii_lowercase();
    let exact_suffix = lower.ends_with("[y/n]")
        || lower.ends_with("(yes/no)")
        || lower.ends_with("password:")
        || lower.ends_with("press any key")
        || line.ends_with("? ");
    exact_suffix.then(|| trimmed.to_string())
}

fn classify_line(line: &str) -> Option<EventKind> {
    let lower = line.to_ascii_lowercase();
    let error = line.contains("Traceback")
        || lower.contains("panicked at")
        || lower.contains("error:")
        || lower.contains("cuda out of memory")
        || line.trim() == "Killed";
    if error {
        return Some(EventKind::ErrorDetected {
            message: line.to_string(),
        });
    }
    lower
        .contains("warning")
        .then(|| EventKind::WarningDetected {
            message: line.to_string(),
        })
}
