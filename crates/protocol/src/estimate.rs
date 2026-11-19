//! How long a running Command has left, from whatever it shows (B1).
//!
//! The watcher picks the best signal there is: the program's own counts
//! (any progress Event: tqdm, `[i/n]`, `epoch 3/40`, `::sentinal progress`,
//! an agent's plan), else how long the same command took before. It is an
//! estimate, said as one, with its basis and confidence; it never changes a
//! Session's state and never notifies.

use crate::Timestamp;
use serde::{Deserialize, Serialize};

/// What an estimate rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EstimateBasis {
    /// The program's own counts and their recent pace.
    Counts,
    /// How long earlier successful runs of the same command took.
    History,
}

/// How much an estimate can be trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EstimateConfidence {
    /// Few points, or an uneven pace: a rough guess.
    Low,
    /// Enough points at a fairly steady pace.
    Medium,
    /// Many points at a steady pace.
    High,
}

/// The time left for a running Command, and why.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct ProgressEstimate {
    /// What it rests on.
    pub basis: EstimateBasis,
    /// How far to trust it.
    pub confidence: EstimateConfidence,
    /// Seconds left from now, rounded; absent when it cannot be said.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_secs: Option<u32>,
    /// Units done per minute at the recent pace (counts only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_minute: Option<f32>,
    /// When the counts last moved (counts only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_progress_at: Option<Timestamp>,
    /// The counts have not moved for this long, far longer than their usual
    /// gap: the run may be stuck.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stalled_secs: Option<u32>,
    /// How long earlier successful runs usually took (median).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typical_secs: Option<u32>,
    /// How many earlier runs `typical_secs` comes from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<u32>,
    /// Already well past how long it usually takes.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub longer_than_usual: bool,
}
