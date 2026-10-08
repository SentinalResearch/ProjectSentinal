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
    /// Total seen per progress unit; a changed total starts a new run.
    totals: Vec<(String, u64)>,
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
            totals: Vec::new(),
        }
    }

    /// A unit's total changed (e.g. a printed sample said 10000 steps, the real
    /// run says 759): numbers read so far belong to something else.
    fn start_series_if_new(&mut self, unit: &str, total: u64) {
        match self.totals.iter_mut().find(|(known, _)| known == unit) {
            Some((_, known_total)) if *known_total == total => {}
            Some((_, known_total)) => {
                *known_total = total;
                self.reset_scalars();
            }
            None => self.totals.push((unit.to_owned(), total)),
        }
    }

    fn reset_scalars(&mut self) {
        self.state.last_loss = None;
        self.state.first_loss = None;
        self.state.best_val_loss = None;
        self.state.learning_rate = None;
        self.state.trend = LossTrend::Unknown;
        self.last_val_loss = None;
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
            self.start_series_if_new(&unit, total);
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
    for (label, unit) in [("epoch", "epoch"), ("step", "step"), ("iter ", "iter")] {
        if let Some(index) = lower.find(label) {
            if let Some((current, total)) = first_fraction(&line[index + label.len()..]) {
                return Some((current, total, unit.into()));
            }
            // "120/500 steps": the count written just before the unit.
            if let Some((current, total)) = trailing_fraction(&line[..index]) {
                return Some((current, total, unit.into()));
            }
        }
    }
    bar_fraction(line)
        .or_else(|| keras_fraction(line))
        .map(|(current, total)| (current, total, "step".into()))
}

/// Keras-style `120/500 [=====>....]` starting the line.
fn keras_fraction(line: &str) -> Option<(u64, u64)> {
    let text = strip_ansi(line);
    let text = text.trim_start();
    let (token, rest) = text.split_once(char::is_whitespace)?;
    let bar = rest.trim_start().strip_prefix('[')?;
    if !bar.starts_with(['=', '>', '.']) {
        return None;
    }
    let (current, total) = token.split_once('/')?;
    Some((current.parse().ok()?, total.parse().ok()?))
}

/// `value` without terminal colour/style escape sequences.
fn strip_ansi(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(character);
        }
    }
    out
}

/// `N/M` from a tqdm-style bar (`45%|████| 450/1000 [...]`) whose label is
/// empty or about training. Loading, tokenizing, mapping, downloading and
/// evaluation bars are other work, and a bare `N/M` elsewhere is not progress.
fn bar_fraction(line: &str) -> Option<(u64, u64)> {
    let bar = line.find("%|")?;
    let label_end = line[..bar]
        .trim_end_matches(|character: char| character.is_ascii_digit() || character == '.');
    let label = strip_ansi(label_end)
        .trim()
        .trim_end_matches(':')
        .to_ascii_lowercase();
    // Work around training, even when the label also says "train"
    // ("Tokenizing train dataset", "Adding EOS to train dataset").
    let other_work = [
        "dataset", "tokeniz", "map", "load", "download", "fetch", "eval", "valid", "test",
        "predict", "build", "writ", "sav", "adding", "dropping", "truncat", "filter", "shard",
        "convert", "process",
    ];
    let training = ["train", "epoch", "step", "iter", "batch"];
    if !label.is_empty()
        && (other_work.iter().any(|word| label.contains(word))
            || !training.iter().any(|word| label.contains(word)))
    {
        return None;
    }
    let after = &line[bar + 2..];
    let close = after.find('|')?;
    first_fraction(&after[close + 1..])
}

/// `N/M` ending `value` (ignoring trailing spaces).
fn trailing_fraction(value: &str) -> Option<(u64, u64)> {
    let token = value.trim_end().rsplit(char::is_whitespace).next()?;
    let (current, total) = token.split_once('/')?;
    Some((current.parse().ok()?, total.parse().ok()?))
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
