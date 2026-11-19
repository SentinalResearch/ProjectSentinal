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
                        marked: None,
                        options: Vec::new(),
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
            ObservationKind::ProcessSample(_)
            | ObservationKind::Screen { .. }
            | ObservationKind::Input => Vec::new(),
        }
    }

    fn state(&self) -> AdapterState {
        AdapterState::Generic(self.state.clone())
    }

    fn waiting(&self) -> bool {
        self.state.waiting
    }
}

pub(crate) fn prompt_line(line: &str) -> Option<String> {
    // Real prompts usually leave the cursor after a space ("Continue? [y/N] ").
    let unterminated = line.trim_end_matches(['\r', '\n']);
    // A terminal reply echoed into the output, or part of a drawn box, asks nothing.
    if unterminated.contains("^[")
        || unterminated.contains(['╭', '╮', '╰', '╯', '┌', '┐', '└', '┘', '│'])
    {
        return None;
    }
    let trimmed = unterminated.trim_end();
    let lower = trimmed.to_ascii_lowercase();
    let exact_suffix = lower.ends_with("[y/n]")
        || lower.ends_with("(yes/no)")
        || lower.ends_with("password:")
        || lower.ends_with("press any key")
        || (trimmed.ends_with('?') && trimmed.len() < unterminated.len());
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
    if let Some((port, url)) = server_started_line(line, &lower) {
        return Some(EventKind::ServerStarted { port, url });
    }
    lower
        .contains("warning")
        .then(|| EventKind::WarningDetected {
            message: line.to_string(),
        })
}

/// The port after the ready phrase: `host:8080`, `[::]:8080`, `:8080` or
/// `port 8080`. Text before the phrase (a timestamp) is never read.
fn listening_port(lower: &str) -> Option<u16> {
    let (start, phrase) = READY_PHRASES
        .iter()
        .filter_map(|phrase| lower.find(phrase).map(|at| (at + phrase.len(), *phrase)))
        .min()?;
    // "server started on port" already said the next word is the port.
    let mut previous = phrase.rsplit(' ').next().unwrap_or("");
    for word in lower[start..].split_whitespace() {
        let word = word.trim_end_matches(|c: char| ",.;)}".contains(c));
        let digits = match word.rsplit_once(':') {
            Some((_, port)) => Some(port),
            None if previous == "port" => Some(word),
            None => None,
        };
        if let Some(port) = digits
            .filter(|port| {
                (1..=5).contains(&port.len()) && port.bytes().all(|b| b.is_ascii_digit())
            })
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port > 0)
        {
            return Some(port);
        }
        previous = word;
    }
    None
}

const READY_PHRASES: &[&str] = &[
    "listening on",
    "listening at",
    "uvicorn running on",
    "server is listening",
    "server started on port",
];

/// Recognise common server-start lines without a dedicated adapter (B1):
/// `listening on :8080`, `Uvicorn running on http://0.0.0.0:8000`,
/// `server started on port 3000`.
fn server_started_line(line: &str, lower: &str) -> Option<(Option<u16>, Option<String>)> {
    if !READY_PHRASES.iter().any(|phrase| lower.contains(phrase)) {
        return None;
    }
    let url = line
        .split_whitespace()
        .find(|part| part.starts_with("http://") || part.starts_with("https://"))
        .map(|part| {
            part.trim_end_matches(|c: char| ",.;)]}".contains(c))
                .to_string()
        });
    let port = url
        .as_deref()
        .and_then(|u| {
            u.split("//")
                .nth(1)?
                .split('/')
                .next()?
                .rsplit_once(':')?
                .1
                .parse()
                .ok()
        })
        .or_else(|| listening_port(lower));
    (port.is_some() || url.is_some()).then_some((port, url))
}
