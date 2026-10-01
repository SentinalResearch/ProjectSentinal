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

/// Result of applying one observation to an Event engine.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineUpdate {
    /// Newly emitted, rate-limited Events.
    pub events: Vec<EventKind>,
    /// Current deterministic Health.
    pub health: Health,
    /// Current state of the Command-specific adapter, when selected.
    pub adapter_state: Option<AdapterState>,
}

/// Pure per-Session engine combining Generic and Command-specific adapters.
pub struct EventEngine {
    generic: GenericAdapter,
    selected: Option<Box<dyn WatcherAdapter>>,
    declarative: Vec<DeclarativeAdapter>,
    health: HealthTracker,
    last_progress: Option<(Timestamp, Option<f32>)>,
    processes: BTreeMap<u32, String>,
    capture: CaptureLevel,
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
            processes: BTreeMap::new(),
            capture: CaptureLevel::Full,
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

    /// Change the capture level, e.g. when an adopted log tail starts or stops.
    pub fn set_capture(&mut self, capture: CaptureLevel) {
        self.capture = capture;
    }

    /// Convert one observation into Events, Health, and adapter state.
    pub fn observe(&mut self, observation: &Observation) -> EngineUpdate {
        if let ObservationKind::CommandStarted(command) = &observation.kind {
            self.select(command);
            self.last_progress = None;
        }
        let capture = self.capture;
        let mut events = self.process_events(observation);
        events.extend(self.generic.observe(observation));
        if let Some(adapter) = &mut self.selected {
            events.extend(adapter.observe(observation));
        }
        events = deduplicate(events)
            .into_iter()
            .filter(|event| capture.supports_evidence(event))
            .filter(|event| self.allow_event(observation.at, event))
            .collect();
        let health = self.health.observe(observation, &events);
        let health = bound(capture, health);
        let adapter_state = self.selected.as_ref().map(|adapter| adapter.state());
        EngineUpdate {
            events,
            health,
            adapter_state,
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
        let current = sample
            .processes
            .iter()
            .map(|process| (process.pid, process.name.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut events = current
            .iter()
            .filter(|(pid, _)| !self.processes.contains_key(pid))
            .map(|(pid, name)| EventKind::ProcessStarted {
                pid: *pid,
                name: name.clone(),
            })
            .collect::<Vec<_>>();
        events.extend(
            self.processes
                .keys()
                .filter(|pid| !current.contains_key(pid))
                .map(|pid| EventKind::ProcessExited {
                    pid: *pid,
                    exit_code: None,
                }),
        );
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
