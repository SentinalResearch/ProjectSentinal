//! Generic plus selected adapter orchestration and Event rate limiting.

use crate::{
    cargo_adapter::CargoAdapter, pytest::PytestAdapter, server::ServerAdapter,
    training::TrainingAdapter, AdapterState, DeclarativeAdapter, GenericAdapter, HealthTracker,
    Observation, ObservationKind, WatcherAdapter,
};
use protocol::{CaptureLevel, Command, DurationMs, EventKind, Health, HealthOverall, Timestamp};
use std::collections::BTreeMap;

const DEFAULT_STALL_MS: u64 = 10 * 60 * 1_000;
const PROGRESS_INTERVAL_MS: i64 = 5_000;
/// Distinct `::sentinal metric` names one Command may report.
const MAX_METRIC_NAMES: usize = 32;

/// Result of applying one observation to an Event engine.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineUpdate {
    /// Newly emitted, rate-limited Events.
    pub events: Vec<EventKind>,
    /// Current deterministic Health.
    pub health: Health,
    /// Current state of the Command-specific adapter, when selected.
    pub adapter_state: Option<AdapterState>,
    /// Whether the program is waiting for a person (a prompt, 6.1).
    pub waiting: bool,
    /// A `::sentinal task` or `state` line from this observation (6.5).
    pub agent_line: Option<crate::progress::AgentLine>,
    /// Time left for the running Command, from its counts or history (B1).
    pub estimate: Option<protocol::ProgressEstimate>,
}

/// A child process that appears after the first sample is reported once it
/// has lived this long.
const MIN_PROCESS_LIFE_MS: i64 = 5_000;

/// Pure per-Session engine combining Generic and Command-specific adapters.
pub struct EventEngine {
    generic: GenericAdapter,
    selected: Option<Box<dyn WatcherAdapter>>,
    declarative: Vec<DeclarativeAdapter>,
    health: HealthTracker,
    last_progress: Option<(Timestamp, Option<f32>)>,
    last_metric: std::collections::HashMap<String, Timestamp>,
    /// pid → (name, first seen, whether its start was reported).
    processes: BTreeMap<u32, (String, i64, bool)>,
    /// A process sample has been seen: what runs at the first one is reported at once.
    sampled: bool,
    capture: CaptureLevel,
    command_activity_required: bool,
    command_active: bool,
    screen: crate::screen_prompt::ScreenPrompts,
    /// Set when the program printed a `::sentinal` API line; explicit lines are
    /// then trusted over adapter and fallback guesses.
    explicit_api: bool,
    /// How many times the daemon started this engine over after a panic.
    restarts: u32,
    /// Warning and error bursts (T0.11).
    bursts: crate::bursts::Bursts,
    /// Server ports already reported for this Command (T0.4).
    servers: std::collections::HashSet<Option<u16>>,
    /// Time left for the running Command (B1).
    estimator: crate::estimate::Estimator,
}

impl EventEngine {
    /// Create an engine and select an adapter for an optional running Command.
    pub fn new(command: Option<&Command>, declarative: Vec<DeclarativeAdapter>) -> Self {
        let mut engine = EventEngine {
            generic: GenericAdapter::new(DurationMs(DEFAULT_STALL_MS)),
            selected: None,
            declarative,
            health: HealthTracker::new(),
            last_progress: None,
            last_metric: std::collections::HashMap::new(),
            processes: BTreeMap::new(),
            sampled: false,
            capture: CaptureLevel::Full,
            command_activity_required: false,
            command_active: command.is_some(),
            screen: crate::screen_prompt::ScreenPrompts::default(),
            explicit_api: false,
            restarts: 0,
            bursts: crate::bursts::Bursts::default(),
            servers: std::collections::HashSet::new(),
            estimator: crate::estimate::Estimator::default(),
        };
        if let Some(command) = command {
            engine.select(command);
        }
        engine
    }

    /// Limit Events and Health to what `capture` can observe (FULL by default).
    pub fn with_capture(mut self, capture: CaptureLevel) -> Self {
        self.capture = capture;
        self
    }

    /// Require a tracked running Command before quiet timers may emit Events.
    pub fn with_command_activity_required(mut self) -> Self {
        self.command_activity_required = true;
        self
    }

    /// A fresh engine with the same adapters and settings, for the daemon to
    /// use after this one panicked (T0.3). The running Command's adapter is
    /// chosen again when the next Command starts.
    pub fn restarted(&self) -> Self {
        let mut engine =
            EventEngine::new(None, self.declarative.clone()).with_capture(self.capture);
        engine.command_activity_required = self.command_activity_required;
        engine.restarts = self.restarts.saturating_add(1);
        engine
    }

    /// How long earlier successful runs of the running command took (B1).
    pub fn set_history(&mut self, durations_ms: &[i64]) {
        self.estimator.set_history(durations_ms);
    }

    /// Times this engine was started over after a panic.
    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    /// The capture level Events and Health are limited to.
    pub fn capture(&self) -> CaptureLevel {
        self.capture
    }

    /// Whether quiet timers wait for a tracked running Command.
    pub fn command_activity_required(&self) -> bool {
        self.command_activity_required
    }

    /// Change the capture level, e.g. when an adopted log tail starts or stops.
    pub fn set_capture(&mut self, capture: CaptureLevel) {
        self.capture = capture;
    }

    /// Convert one observation into Events, Health, and adapter state.
    pub fn observe(&mut self, observation: &Observation) -> EngineUpdate {
        if let ObservationKind::CommandStarted(command) = &observation.kind {
            self.select(command);
            self.last_progress = None;
            self.last_metric.clear();
            self.command_active = true;
            self.screen.reset();
            self.explicit_api = false;
            self.bursts.reset();
            self.servers.clear();
            self.estimator.start(observation.at);
        } else if matches!(observation.kind, ObservationKind::CommandFinished(_)) {
            self.command_active = false;
            self.screen.reset();
            self.estimator.finish();
        }
        let capture = self.capture;
        let mut events = self.process_events(observation);
        let mut agent_line = None;
        // Explicit ::sentinal API lines are parsed first, ahead of adapters
        // and the fallback counter. Once a Session prints one, it is trusted
        // over guesses for that Session.
        if let ObservationKind::OutputLine(line) = &observation.kind {
            if let Some(explicit) = crate::progress::explicit_line(line) {
                self.explicit_api = true;
                match explicit {
                    crate::progress::ExplicitLine::Progress {
                        current,
                        total,
                        unit,
                    } => {
                        events.push(EventKind::ProgressUpdated {
                            current,
                            total,
                            unit,
                        });
                    }
                    crate::progress::ExplicitLine::Metric { name, value } => {
                        events.push(EventKind::MetricReported { name, value });
                    }
                    crate::progress::ExplicitLine::Agent(line) => agent_line = Some(line),
                }
            }
        }
        if matches!(observation.kind, ObservationKind::Input) {
            self.screen.input();
        }
        if let ObservationKind::Screen { rows, alternate } = &observation.kind {
            if self.command_active {
                events.extend(self.screen.observe(observation.at, rows, *alternate));
            }
        }
        // In a full-screen program lines are fragments of redraws; while the
        // screen shows a question it is the only one being asked.
        let lines_mean_nothing = self.screen.alternate() || self.screen.waiting();
        let quiet_timer = matches!(
            observation.kind,
            ObservationKind::MetricsSample(_) | ObservationKind::Tick
        );
        if !(self.command_activity_required && !self.command_active && quiet_timer) {
            let owns_prompts = self
                .selected
                .as_ref()
                .is_some_and(|adapter| adapter.owns_prompts());
            events.extend(
                self.generic
                    .observe(observation)
                    .into_iter()
                    .filter(|event| {
                        !((owns_prompts || lines_mean_nothing)
                            && matches!(event, EventKind::WaitingForInput { .. }))
                    }),
            );
        }
        let question_on_screen = self.screen.waiting();
        match (&mut self.selected, &observation.kind) {
            (Some(adapter), _) => {
                adapter.set_question_on_screen(question_on_screen);
                events.extend(adapter.observe(observation));
            }
            // No adapter knows this program: explicit counters still count,
            // unless the program already used the ::sentinal API.
            (None, ObservationKind::OutputLine(line)) if !self.explicit_api => {
                if let Some((current, total, unit)) = crate::progress::fallback_progress(line) {
                    events.push(EventKind::ProgressUpdated {
                        current,
                        total: Some(total),
                        unit,
                    });
                }
            }
            _ => {}
        }
        // Every count feeds the estimate, rate-limited Events or not.
        for event in &events {
            if let EventKind::ProgressUpdated { current, total, .. } = event {
                self.estimator.progress(observation.at, *current, *total);
            }
        }
        let summaries = self.bursts.summaries(observation.at);
        let mut counted = summaries.events;
        counted.extend(
            deduplicate(events)
                .into_iter()
                .filter(|event| capture.supports_evidence(event))
                .filter(|event| self.allow_event(observation.at, event)),
        );
        events = counted;
        let adapter_state = self.selected.as_ref().map(|adapter| adapter.state());
        let waiting = self.screen.waiting()
            || match &self.selected {
                Some(adapter) if adapter.owns_prompts() => adapter.waiting(),
                _ => !lines_mean_nothing && self.generic.waiting(),
            };
        let asked_now = events
            .iter()
            .any(|event| matches!(event, EventKind::WaitingForInput { .. }));
        // T0.6: an answered question stops warning; one asked just now counts.
        self.health.set_waiting(waiting || asked_now);
        // Health counts every warning and error the program printed, not
        // only the Events kept from a burst.
        self.health
            .count_left_out(summaries.warnings, summaries.errors);
        let health = self.health.observe(observation, &events);
        let health = bound(capture, health);
        EngineUpdate {
            events,
            health,
            adapter_state,
            waiting,
            agent_line,
            estimate: self.estimator.estimate(observation.at),
        }
    }

    /// Apply an Event produced outside watcher adapters to deterministic Health.
    pub fn observe_existing_event(&mut self, at: Timestamp, event: &EventKind) -> Health {
        let supported = self.capture.supports_evidence(event);
        let health = self.health.observe(
            &Observation {
                at,
                kind: ObservationKind::Tick,
            },
            if supported {
                std::slice::from_ref(event)
            } else {
                &[]
            },
        );
        bound(self.capture, health)
    }

    fn process_events(&mut self, observation: &Observation) -> Vec<EventKind> {
        let ObservationKind::ProcessSample(sample) = &observation.kind else {
            return Vec::new();
        };
        let at = observation.at.0;
        let first = !self.sampled;
        self.sampled = true;
        // A terminal that tracks Commands already says what started and
        // ended; one Event per child process (every `sleep`, every tool an
        // agent runs) only buries them (QA 2026-11-19).
        let quiet = self.command_activity_required;
        let mut current: BTreeMap<u32, (String, i64, bool)> = BTreeMap::new();
        let mut events = Vec::new();
        for process in &sample.processes {
            let known = self.processes.get(&process.pid);
            let first_seen = known.map_or(at, |(_, seen, _)| *seen);
            let reported = known.is_some_and(|(_, _, reported)| *reported);
            // Adopted runs: what runs when first seen is the run itself; a
            // child counts once it has lived a while, so short-lived workers
            // do not flood the Events (field report 2026-11-19).
            let report = !quiet
                && !reported
                && (first || at.saturating_sub(first_seen) >= MIN_PROCESS_LIFE_MS);
            if report {
                events.push(EventKind::ProcessStarted {
                    pid: process.pid,
                    name: process.name.clone(),
                });
            }
            current.insert(
                process.pid,
                (process.name.clone(), first_seen, reported || report),
            );
        }
        for (pid, (_, _, reported)) in &self.processes {
            if *reported && !current.contains_key(pid) {
                events.push(EventKind::ProcessExited {
                    pid: *pid,
                    exit_code: None,
                });
            }
        }
        self.processes = current;
        events
    }

    fn select(&mut self, command: &Command) {
        if let Some(adapter) = self
            .declarative
            .iter()
            .find(|adapter| adapter.matches(command))
            .cloned()
        {
            self.selected = Some(Box::new(adapter));
            return;
        }
        let builtins: Vec<Box<dyn WatcherAdapter>> = vec![
            Box::new(crate::AgentCliAdapter::new()),
            Box::new(TrainingAdapter::new()),
            Box::new(PytestAdapter::new()),
            Box::new(ServerAdapter::new()),
            Box::new(CargoAdapter::new()),
        ];
        self.selected = builtins
            .into_iter()
            .find(|adapter| adapter.matches(command));
    }

    fn allow_event(&mut self, at: Timestamp, event: &EventKind) -> bool {
        if !self.bursts.admit(at, event) {
            return false;
        }
        // A server is news once per port per Command, not on every reload.
        if let EventKind::ServerStarted { port, .. } = event {
            return self.servers.insert(*port);
        }
        if let EventKind::MetricReported { name, .. } = event {
            // Like progress: at most one Event per metric every few seconds,
            // and only so many distinct names per Command.
            let known = self.last_metric.contains_key(name);
            if !known && self.last_metric.len() >= MAX_METRIC_NAMES {
                return false;
            }
            let allowed = self
                .last_metric
                .get(name)
                .is_none_or(|previous| at.0.saturating_sub(previous.0) >= PROGRESS_INTERVAL_MS);
            if allowed {
                self.last_metric.insert(name.clone(), at);
            }
            return allowed;
        }
        let EventKind::ProgressUpdated { .. } = event else {
            return true;
        };
        let percent = event.progress_percent();
        let allowed = self
            .last_progress
            .is_none_or(|(previous_at, previous_percent)| {
                at.0.saturating_sub(previous_at.0) >= PROGRESS_INTERVAL_MS
                    || percent
                        .zip(previous_percent)
                        .is_some_and(|(current, previous)| (current - previous).abs() >= 1.0)
            });
        if allowed {
            self.last_progress = Some((at, percent));
        }
        allowed
    }
}

/// Without exit-status evidence, error lines are a warning, never a failure.
fn bound(capture: CaptureLevel, mut health: Health) -> Health {
    if health.overall == HealthOverall::Failed && !capture.supports_failure_evidence() {
        health.overall = HealthOverall::Warning;
    }
    health
}

fn deduplicate(events: Vec<EventKind>) -> Vec<EventKind> {
    let mut unique = Vec::new();
    for event in events {
        if !unique.contains(&event) {
            unique.push(event);
        }
    }
    unique
}
