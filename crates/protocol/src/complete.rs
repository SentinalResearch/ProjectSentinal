//! Autocomplete (Milestone 5.A): the request, its completions, and the
//! frecency formula. No model is involved.
//!
//! For each distinct Command text that starts with the prefix:
//!
//! ```text
//! count    = successful runs (all finished runs when failures are included)
//! frecency = count × 0.5 ^ (days since last run / half_life_days)
//! score    = frecency_weight × frecency
//!          + project_bonus  × share of runs inside the current project
//!          + cwd_bonus      × share of runs in the current directory
//!          + success_bonus  × share of runs that succeeded
//!          − failure_penalty × share that failed   (only when included)
//! ```
//!
//! Commands that never succeeded are left out unless failures are asked for.
//! Recipes score a flat `recipe`, below any Command with real history.
//! Weights come from `[complete]` in `daemon.toml`.

use crate::Timestamp;
use serde::{Deserialize, Serialize};

/// Longest prefix accepted, in UTF-8 bytes.
pub const MAX_COMPLETE_PREFIX_BYTES: usize = 1_024;
/// Most completions returned.
pub const MAX_COMPLETE_LIMIT: u32 = 20;
const MAX_CWD_BYTES: usize = 4_096;
const DAY_MS: f64 = 86_400_000.0;

/// Where a completion came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CompletionSource {
    /// A Command run on this Machine.
    History,
    /// A Recipe's command with its default values.
    Recipe,
}

/// One ranked completion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Completion {
    /// The whole Command line, which starts with the prefix.
    pub text: String,
    /// History or Recipe.
    pub source: CompletionSource,
    /// Runs counted (0 for a Recipe).
    pub count: u32,
    /// Last run, if it has run.
    pub last_at: Option<Timestamp>,
    /// Exit code of the last finished run.
    pub last_exit: Option<i32>,
    /// [`completion_score`]; higher is better.
    pub score: f64,
}

/// Formula weights (`[complete]` in `daemon.toml`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CompletionWeights {
    /// Multiplies frecency.
    pub frecency_weight: f64,
    /// Days for a run's weight to halve.
    pub half_life_days: f64,
    /// Added in proportion to runs inside the current project.
    pub project_bonus: f64,
    /// Added in proportion to runs in the current directory.
    pub cwd_bonus: f64,
    /// Added in proportion to runs that succeeded.
    pub success_bonus: f64,
    /// Subtracted in proportion to runs that failed, when failures are included.
    pub failure_penalty: f64,
    /// Flat score of a matching Recipe.
    pub recipe: f64,
}

impl Default for CompletionWeights {
    fn default() -> Self {
        CompletionWeights {
            frecency_weight: 1.0,
            half_life_days: 7.0,
            project_bonus: 10.0,
            cwd_bonus: 5.0,
            success_bonus: 2.0,
            failure_penalty: 10.0,
            recipe: 1.0,
        }
    }
}

impl CompletionWeights {
    /// Weights must be finite and not negative, with a positive half-life.
    pub fn validate(&self) -> Result<(), String> {
        let all = [
            self.frecency_weight,
            self.half_life_days,
            self.project_bonus,
            self.cwd_bonus,
            self.success_bonus,
            self.failure_penalty,
            self.recipe,
        ];
        if all
            .iter()
            .any(|weight| !weight.is_finite() || *weight < 0.0)
        {
            return Err("completion weights must be finite and not negative".into());
        }
        if self.half_life_days <= 0.0 {
            return Err("half_life_days must be positive".into());
        }
        Ok(())
    }
}

/// What one Command text's history adds up to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CandidateStats {
    /// Runs that exited 0.
    pub succeeded: u32,
    /// Runs that exited non-zero.
    pub failed: u32,
    /// Start of the latest run.
    pub last_at: Timestamp,
    /// Finished runs whose cwd is inside the current project.
    pub in_project: u32,
    /// Finished runs in the current directory.
    pub in_cwd: u32,
}

/// `count × 0.5 ^ (age / half-life)`, times the frecency weight.
pub fn frecency(weights: &CompletionWeights, count: u32, age_ms: i64) -> f64 {
    let age_days = age_ms.max(0) as f64 / DAY_MS;
    weights.frecency_weight * f64::from(count) * 0.5_f64.powf(age_days / weights.half_life_days)
}

/// The score in the module documentation, rounded to hundredths; 0 means "leave out".
pub fn completion_score(
    weights: &CompletionWeights,
    stats: &CandidateStats,
    now: Timestamp,
    include_failed: bool,
) -> f64 {
    let finished = stats.succeeded + stats.failed;
    let count = if include_failed {
        finished
    } else {
        stats.succeeded
    };
    if count == 0 {
        return 0.0;
    }
    let share = |part: u32| f64::from(part) / f64::from(finished);
    let mut score = frecency(weights, count, now.0.saturating_sub(stats.last_at.0))
        + weights.project_bonus * share(stats.in_project)
        + weights.cwd_bonus * share(stats.in_cwd)
        + weights.success_bonus * share(stats.succeeded);
    if include_failed {
        score -= weights.failure_penalty * share(stats.failed);
    }
    // Still listed, however often it failed: the owner asked for failures.
    ((score.max(0.01)) * 100.0).round() / 100.0
}

/// Check the bounds shared by the WebSocket and local-socket requests.
pub(crate) fn validate_request(
    prefix: &str,
    cwd: Option<&str>,
    limit: u32,
) -> Result<(), crate::ProtocolError> {
    if prefix.len() > MAX_COMPLETE_PREFIX_BYTES {
        return Err(crate::ProtocolError::FieldTooLong {
            field: "prefix",
            actual: prefix.len(),
            maximum: MAX_COMPLETE_PREFIX_BYTES,
        });
    }
    if let Some(cwd) = cwd.filter(|cwd| cwd.len() > MAX_CWD_BYTES) {
        return Err(crate::ProtocolError::FieldTooLong {
            field: "cwd",
            actual: cwd.len(),
            maximum: MAX_CWD_BYTES,
        });
    }
    if !(1..=MAX_COMPLETE_LIMIT).contains(&limit) {
        return Err(crate::ProtocolError::InvalidQueryLimit {
            field: "complete",
            actual: limit,
            maximum: MAX_COMPLETE_LIMIT,
        });
    }
    Ok(())
}
