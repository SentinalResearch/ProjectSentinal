//! Deterministic Health derived from observations and Events.

use crate::{Observation, ObservationKind};
use protocol::{EventKind, Health, HealthLevel, HealthOverall};

const MAX_EVIDENCE: usize = 16;
const MAX_EVIDENCE_BYTES: usize = 512;

/// Stateful deterministic Health calculator.
pub struct HealthTracker {
    health: Health,
    stalled: bool,
    warnings: u32,
}

impl HealthTracker {
    /// Create a tracker with unknown dimensions and a healthy overall state.
    pub fn new() -> Self {
        HealthTracker {
            health: Health {
                progress: None,
                activity: HealthLevel::Unknown,
                resources: HealthLevel::Unknown,
                stability: HealthLevel::Unknown,
                errors: 0,
                overall: HealthOverall::Healthy,
                evidence: Vec::new(),
            },
            stalled: false,
            warnings: 0,
        }
    }

    /// Apply one observation and its emitted Events, returning current Health.
    pub fn observe(&mut self, observation: &Observation, events: &[EventKind]) -> Health {
        match &observation.kind {
            ObservationKind::OutputLine(_) => {
                self.health.activity = HealthLevel::Good;
                self.stalled = false;
                if events.is_empty() {
                    self.add_evidence("terminal output observed");
                }
            }
            ObservationKind::MetricsSample(metrics) => self.apply_metrics(metrics),
            ObservationKind::CommandStarted(_) => {
                self.health.activity = HealthLevel::Good;
                self.add_evidence("Command is running");
            }
            ObservationKind::CommandFinished(command) => {
                if command.exit_code == Some(0) {
                    self.health.activity = HealthLevel::Good;
                    self.add_evidence("Command finished successfully");
                } else if let Some(code) = command.exit_code {
                    self.health.errors = self.health.errors.saturating_add(1);
                    self.health.stability = HealthLevel::Critical;
                    self.add_evidence(&format!("Command finished with exit code {code}"));
                }
            }
            ObservationKind::ProcessSample(_) | ObservationKind::Tick => {}
        }
        for event in events {
            self.apply_event(event);
        }
        self.recompute_overall();
        self.health.clone()
    }

    fn apply_metrics(&mut self, metrics: &crate::MetricsObservation) {
        // Per-core CPU can exceed 100% on a multi-core Machine; pressure is
        // judged by the share of the whole Machine.
        let peak = [
            metrics.cpu_machine_percent,
            metrics.gpu_percent,
            metrics.ram_percent,
            metrics.vram_percent,
        ]
        .into_iter()
        .flatten()
        .fold(0.0_f32, f32::max);
        self.health.resources = if peak >= 95.0 {
            HealthLevel::Critical
        } else if peak >= 90.0 {
            HealthLevel::Warning
        } else {
            HealthLevel::Good
        };
        if peak >= 90.0 {
            self.add_evidence(&format!("resource usage reached {peak:.1}%"));
        } else {
            self.add_evidence("resources below 90%");
        }
    }

    fn apply_event(&mut self, event: &EventKind) {
        match event {
            EventKind::ProgressUpdated { .. } => {
                self.health.progress = event.progress_percent();
                self.health.activity = HealthLevel::Good;
                if let Some(percent) = self.health.progress {
                    self.add_evidence(&format!("progress reached {percent:.1}%"));
                } else {
                    self.add_evidence("progress advanced");
                }
            }
            EventKind::WarningDetected { message } => {
                self.warnings = self.warnings.saturating_add(1);
                self.health.stability = HealthLevel::Warning;
                self.add_evidence(message);
            }
            EventKind::ErrorDetected { message } => {
                self.health.errors = self.health.errors.saturating_add(1);
                self.health.stability = HealthLevel::Critical;
                self.add_evidence(message);
            }
            EventKind::ResourceHigh { resource, percent } => {
                self.health.resources = if *percent >= 95.0 {
                    HealthLevel::Critical
                } else {
                    HealthLevel::Warning
                };
                self.add_evidence(&format!("{resource:?} usage reached {percent:.1}%"));
            }
            EventKind::WaitingForInput { prompt } => {
                self.health.activity = HealthLevel::Warning;
                self.warnings = self.warnings.saturating_add(1);
                self.add_evidence(
                    prompt
                        .as_deref()
                        .unwrap_or("Command may be waiting for input"),
                );
            }
            EventKind::PossiblyStalled { silent_for_ms } => {
                self.stalled = true;
                self.health.activity = HealthLevel::Warning;
                self.health.stability = HealthLevel::Warning;
                self.add_evidence(&format!(
                    "quiet with low resources for {} ms",
                    silent_for_ms.0
                ));
            }
            EventKind::CommandFinished { exit_code, .. } if *exit_code != 0 => {
                self.health.errors = self.health.errors.saturating_add(1);
                self.health.stability = HealthLevel::Critical;
                self.add_evidence(&format!("Command failed with exit code {exit_code}"));
            }
            _ => {}
        }
    }

    fn recompute_overall(&mut self) {
        self.health.overall = if self.health.errors > 0 {
            HealthOverall::Failed
        } else if self.stalled {
            HealthOverall::PossiblyStalled
        } else if self.warnings > 0
            || matches!(
                self.health.resources,
                HealthLevel::Warning | HealthLevel::Critical
            )
        {
            HealthOverall::Warning
        } else {
            HealthOverall::Healthy
        };
        if self.health.evidence.is_empty() {
            self.add_evidence("No adverse evidence observed");
        }
    }

    fn add_evidence(&mut self, value: &str) {
        let bounded = crate::observation::truncate_utf8(value, MAX_EVIDENCE_BYTES);
        if self.health.evidence.last() == Some(&bounded) {
            return;
        }
        if self.health.evidence.len() == MAX_EVIDENCE {
            self.health.evidence.remove(0);
        }
        self.health.evidence.push(bounded);
    }
}

impl Default for HealthTracker {
    fn default() -> Self {
        Self::new()
    }
}
