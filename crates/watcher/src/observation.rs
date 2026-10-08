//! Bounded structured inputs consumed by watcher adapters.

use protocol::{Command, Process, Timestamp};

/// Maximum UTF-8 bytes retained from one output line.
pub const MAX_OUTPUT_LINE_BYTES: usize = 8 * 1024;

/// Resource values needed for deterministic watcher decisions.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricsObservation {
    /// Aggregate Session CPU as a sum of per-core percentages (can exceed 100);
    /// used to tell an active process from an idle one.
    pub cpu_percent: f32,
    /// Session CPU as a share of the whole Machine (0–100), when the core
    /// count is known; used for resource pressure.
    pub cpu_machine_percent: Option<f32>,
    /// Aggregate GPU utilization when a Session uses a GPU.
    pub gpu_percent: Option<f32>,
    /// Session RAM as a percentage of Machine RAM when known.
    pub ram_percent: Option<f32>,
    /// Session VRAM as a percentage of Machine VRAM when known.
    pub vram_percent: Option<f32>,
}

/// One process-tree observation for a Session.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessObservation {
    /// Processes currently in the Session tree.
    pub processes: Vec<Process>,
}

/// Kind-specific data supplied to adapters.
#[derive(Debug, Clone, PartialEq)]
pub enum ObservationKind {
    /// One ANSI-free bounded terminal or log line.
    OutputLine(String),
    /// Current process tree.
    ProcessSample(ProcessObservation),
    /// Current resource sample.
    MetricsSample(MetricsObservation),
    /// A structured Command began.
    CommandStarted(Command),
    /// A structured Command finished.
    CommandFinished(Command),
    /// A clock tick used for quiet-time decisions.
    Tick,
}

/// One timestamped structured watcher input.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// Observation time in Unix milliseconds.
    pub at: Timestamp,
    /// Observed value.
    pub kind: ObservationKind,
}

impl Observation {
    /// Build one bounded output-line observation.
    pub fn output(at: Timestamp, line: impl AsRef<str>) -> Self {
        Observation {
            at,
            kind: ObservationKind::OutputLine(truncate_utf8(line.as_ref(), MAX_OUTPUT_LINE_BYTES)),
        }
    }

    /// Build one resource observation with CPU and GPU utilization.
    pub fn metrics(at: Timestamp, cpu_percent: f32, gpu_percent: Option<f32>) -> Self {
        Observation {
            at,
            kind: ObservationKind::MetricsSample(MetricsObservation {
                cpu_percent,
                cpu_machine_percent: None,
                gpu_percent,
                ram_percent: None,
                vram_percent: None,
            }),
        }
    }

    /// Build a quiet-time clock tick.
    pub fn tick(at: Timestamp) -> Self {
        Observation {
            at,
            kind: ObservationKind::Tick,
        }
    }
}

pub(crate) fn truncate_utf8(value: &str, maximum: usize) -> String {
    if value.len() <= maximum {
        return value.to_string();
    }
    let mut end = maximum;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value[..end].to_string()
}
