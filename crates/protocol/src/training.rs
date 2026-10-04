//! Structured training state the watcher reads from a run's output.

use serde::{Deserialize, Serialize};

/// Direction of recent validation-loss movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LossTrend {
    /// No pair of validation values has been observed.
    Unknown,
    /// Validation loss decreased.
    Improving,
    /// Validation loss did not materially move.
    Stable,
    /// Validation loss increased.
    Worsening,
}

/// Latest training numbers for a Session, from its printed output only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct TrainingSummary {
    /// Most recent training loss.
    pub loss: Option<f64>,
    /// First training loss seen, for the "from" side of a trend.
    pub first_loss: Option<f64>,
    /// Best validation loss seen.
    pub best_val_loss: Option<f64>,
    /// Most recent learning rate.
    pub learning_rate: Option<f64>,
    /// Validation-loss direction.
    pub trend: LossTrend,
}
