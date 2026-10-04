//! Training progress, scalar, trend, and checkpoint recognition.

use crate::{AdapterState, Observation, ObservationKind, WatcherAdapter};
use protocol::{Command, EventKind, LossTrend, TrainingSummary};

/// Current structured state extracted from training output.
#[derive(Debug, Clone, PartialEq)]
pub struct TrainingState {
    /// Current epoch or step.
    pub current: Option<u64>,
    /// Known total epochs or steps.
    pub total: Option<u64>,
    /// Unit of progress.
    pub unit: Option<String>,
    /// Most recent training loss.
    pub last_loss: Option<f64>,
    /// First training loss seen.
    pub first_loss: Option<f64>,
    /// Best validation loss seen.
    pub best_val_loss: Option<f64>,
    /// Most recent learning rate.
    pub learning_rate: Option<f64>,
    /// Validation-loss direction.
    pub trend: LossTrend,
}

/// Adapter for Python model-training Commands.
pub struct TrainingAdapter {
    state: TrainingState,
    last_val_loss: Option<f64>,
}

impl TrainingAdapter {
    /// Create an empty training adapter.
    pub fn new() -> Self {
        TrainingAdapter {
            state: TrainingState {
                current: None,
                total: None,
                unit: None,
                last_loss: None,
                first_loss: None,
                best_val_loss: None,
                learning_rate: None,
                trend: LossTrend::Unknown,
            },
            last_val_loss: None,
        }
    }
}

impl TrainingAdapter {
    /// The wire summary, once any training number has been read.
    pub fn summary(&self) -> Option<TrainingSummary> {
        self.state.summary()
    }
}

impl TrainingState {
    /// The wire summary, once any training number has been read.
    pub fn summary(&self) -> Option<TrainingSummary> {
        let known = self.last_loss.is_some()
            || self.best_val_loss.is_some()
            || self.learning_rate.is_some();
        known.then_some(TrainingSummary {
            loss: self.last_loss,
            first_loss: self.first_loss,
            best_val_loss: self.best_val_loss,
            learning_rate: self.learning_rate,
            trend: self.trend,
        })
    }
}

impl Default for TrainingAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl WatcherAdapter for TrainingAdapter {
    fn name(&self) -> &'static str {
        "training"
    }

    fn matches(&self, command: &Command) -> bool {
        let lower = command.text.to_ascii_lowercase();
        let runner =
            lower.contains("python") || lower.contains("torchrun") || lower.contains("accelerate");
        runner && lower.contains("train")
    }

    fn observe(&mut self, observation: &Observation) -> Vec<EventKind> {
        let ObservationKind::OutputLine(line) = &observation.kind else {
            return Vec::new();
        };
        let mut events = Vec::new();
        if let Some((current, total, unit)) = progress(line) {
            self.state.current = Some(current);
            self.state.total = Some(total);
            self.state.unit = Some(unit.clone());
            events.push(EventKind::ProgressUpdated {
                current,
                total: Some(total),
                unit,
            });
        }
        if let Some(value) = scalar(line, "val_loss") {
            self.state.trend = match self.last_val_loss {
                Some(previous) if value < previous - f64::EPSILON => LossTrend::Improving,
                Some(previous) if value > previous + f64::EPSILON => LossTrend::Worsening,
                Some(_) => LossTrend::Stable,
                None => LossTrend::Unknown,
            };
            self.last_val_loss = Some(value);
            self.state.best_val_loss = Some(
                self.state
                    .best_val_loss
                    .map_or(value, |best| best.min(value)),
            );
        }
        self.state.last_loss = scalar(line, "loss").or(self.state.last_loss);
        self.state.first_loss = self.state.first_loss.or(self.state.last_loss);
        self.state.learning_rate = scalar(line, "learning_rate")
            .or_else(|| scalar(line, "lr"))
            .or(self.state.learning_rate);
        if line.to_ascii_lowercase().contains("saving checkpoint") {
            if let Some(path) = checkpoint_path(line) {
                events.push(EventKind::CheckpointCreated { path });
            }
        }
        events
    }

    fn state(&self) -> AdapterState {
        AdapterState::Training(self.state.clone())
    }
}

fn progress(line: &str) -> Option<(u64, u64, String)> {
    let lower = line.to_ascii_lowercase();
    for (label, unit) in [("epoch", "epoch"), ("step", "step")] {
        if let Some(index) = lower.find(label) {
            if let Some((current, total)) = first_fraction(&line[index + label.len()..]) {
                return Some((current, total, unit.into()));
            }
        }
    }
    first_fraction(line).map(|(current, total)| (current, total, "step".into()))
}

fn first_fraction(value: &str) -> Option<(u64, u64)> {
    let bytes = value.as_bytes();
    for slash in 1..bytes.len().saturating_sub(1) {
        if bytes[slash] != b'/' {
            continue;
        }
        let mut start = slash;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        let mut end = slash + 1;
        while end < bytes.len() && bytes[end].is_ascii_digit() {
            end += 1;
        }
        if start == slash || end == slash + 1 {
            continue;
        }
        let current = value[start..slash].parse().ok()?;
        let total = value[slash + 1..end].parse().ok()?;
        return Some((current, total));
    }
    None
}

fn scalar(line: &str, key: &str) -> Option<f64> {
    let lower = line.to_ascii_lowercase();
    let mut offset = 0;
    while let Some(found) = lower[offset..].find(key) {
        let index = offset + found;
        if key == "loss" && index >= 4 && &lower[index - 4..index] == "val_" {
            offset = index + key.len();
            continue;
        }
        let rest = &line[index + key.len()..];
        let rest = rest.trim_start_matches(|character: char| {
            character.is_ascii_whitespace() || matches!(character, '=' | ':' | '\'' | '"')
        });
        let end = rest
            .find(|character: char| {
                !(character.is_ascii_digit() || matches!(character, '.' | '-' | '+' | 'e' | 'E'))
            })
            .unwrap_or(rest.len());
        if end > 0 {
            if let Ok(number) = rest[..end].parse() {
                return Some(number);
            }
        }
        offset = index + key.len();
    }
    None
}

fn checkpoint_path(line: &str) -> Option<String> {
    line.split_whitespace()
        .map(|part| part.trim_matches(|value: char| matches!(value, ',' | ':' | '\'' | '"')))
        .find(|part| {
            [".pt", ".safetensors", ".ckpt"]
                .iter()
                .any(|suffix| part.contains(suffix))
        })
        .map(ToOwned::to_owned)
}
