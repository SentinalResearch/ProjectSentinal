//! Time left for the running Command, from whatever it shows (B1).
//!
//! Signals, best first:
//!
//! 1. **Counts**: every progress Event, whoever produced it (tqdm, `[i/n]`,
//!    `epoch 3/40`, `::sentinal progress`, an agent's plan). The pace is the
//!    blend of the median step rate (robust to bursts and pauses) and the
//!    average over the recent window (smooth), over the last
//!    [`WINDOW`] points; a jump back or a new total starts over.
//! 2. **History**: with no counts, the median of earlier successful runs of
//!    the same command in the same folder, which the daemon supplies.
//!
//! Counts that stop for far longer than their usual gap are reported as a
//! stall rather than counted down. Numbers are rounded so the estimate only
//! changes when it changes meaningfully. Pure: no clock, no I/O.

use protocol::{EstimateBasis, EstimateConfidence, ProgressEstimate, Timestamp};
use std::collections::VecDeque;

/// Progress points kept for the pace.
pub const WINDOW: usize = 12;
/// A stall is at least this long, whatever the usual gap.
const MIN_STALL_MS: i64 = 60_000;
/// And at least this many usual gaps.
const STALL_GAPS: f64 = 3.0;
/// Past `typical × this` (and a minute), a run is longer than usual.
const LONGER_FACTOR: f64 = 1.5;

/// One Command's progress points and history.
#[derive(Debug, Default, Clone)]
pub struct Estimator {
    started_at: Option<i64>,
    points: VecDeque<(f64, i64)>,
    total: Option<f64>,
    /// `(median ms, runs, spread)` of earlier successful runs.
    typical: Option<(i64, u32, f64)>,
}

impl Estimator {
    /// A Command started at `at`.
    pub fn start(&mut self, at: Timestamp) {
        *self = Estimator {
            started_at: Some(at.0),
            ..Estimator::default()
        };
    }

    /// The Command ended: nothing is left to estimate.
    pub fn finish(&mut self) {
        *self = Estimator::default();
    }

    /// How long earlier successful runs of this command took, in ms.
    pub fn set_history(&mut self, durations_ms: &[i64]) {
        let mut runs: Vec<i64> = durations_ms.iter().copied().filter(|d| *d > 0).collect();
        if runs.is_empty() {
            self.typical = None;
            return;
        }
        runs.sort_unstable();
        let median = runs[runs.len() / 2];
        let spread = (runs[runs.len() - 1] - runs[0]) as f64 / median.max(1) as f64;
        self.typical = Some((
            median,
            u32::try_from(runs.len()).unwrap_or(u32::MAX),
            spread,
        ));
    }

    /// A progress Event: `current` of `total` at `at`.
    pub fn progress(&mut self, at: Timestamp, current: u64, total: Option<u64>) {
        let current = current as f64;
        let total = total.map(|total| total as f64);
        let restarted =
            self.total != total || self.points.back().is_some_and(|(last, _)| current < *last);
        if restarted {
            self.points.clear();
        }
        self.total = total;
        if self.points.back().is_none_or(|(last, _)| current > *last) {
            self.points.push_back((current, at.0));
            while self.points.len() > WINDOW {
                self.points.pop_front();
            }
        }
    }

    /// The estimate at `now`, if anything can be said.
    pub fn estimate(&self, now: Timestamp) -> Option<ProgressEstimate> {
        let started = self.started_at?;
        let elapsed = now.0.saturating_sub(started);
        let history = self.typical.map(|(median, runs, spread)| {
            let longer =
                elapsed as f64 > median as f64 * LONGER_FACTOR && elapsed - median > 60_000;
            (median, runs, spread, longer)
        });
        if let Some(estimate) = self.by_counts(now, history) {
            return Some(estimate);
        }
        let (median, runs, spread, longer) = history?;
        let left = median - elapsed;
        Some(ProgressEstimate {
            basis: EstimateBasis::History,
            confidence: if runs >= 3 && spread < 0.3 {
                EstimateConfidence::Medium
            } else {
                EstimateConfidence::Low
            },
            left_secs: (left > 0).then(|| round_secs(left)),
            per_minute: None,
            last_progress_at: None,
            stalled_secs: None,
            typical_secs: Some(round_secs(median)),
            runs: Some(runs),
            longer_than_usual: longer,
        })
    }

    fn by_counts(
        &self,
        now: Timestamp,
        history: Option<(i64, u32, f64, bool)>,
    ) -> Option<ProgressEstimate> {
        let total = self.total?;
        let &(last, last_at) = self.points.back()?;
        let pairs: Vec<(f64, f64)> = self
            .points
            .iter()
            .zip(self.points.iter().skip(1))
            .filter(|((_, a), (_, b))| b > a)
            .map(|((c1, t1), (c2, t2))| ((c2 - c1) / (t2 - t1) as f64, (t2 - t1) as f64))
            .collect();
        if pairs.is_empty() {
            return None;
        }
        let mut rates: Vec<f64> = pairs.iter().map(|(rate, _)| *rate).collect();
        let mut gaps: Vec<f64> = pairs.iter().map(|(_, gap)| *gap).collect();
        let median_rate = median(&mut rates);
        let median_gap = median(&mut gaps);
        let &(first, first_at) = self.points.front()?;
        let average = (last - first) / (last_at - first_at).max(1) as f64;
        let rate = 0.5 * median_rate + 0.5 * average;
        if !rate.is_finite() || rate <= 0.0 {
            return None;
        }
        let since = now.0.saturating_sub(last_at);
        let stalled = since as f64 > (median_gap * STALL_GAPS).max(MIN_STALL_MS as f64);
        let remaining = (total - last).max(0.0);
        let left = (remaining / rate) as i64 - since;
        let confidence = confidence(pairs.len(), &rates);
        Some(ProgressEstimate {
            basis: EstimateBasis::Counts,
            confidence,
            left_secs: (!stalled).then(|| round_secs(left.max(0))),
            per_minute: Some(two_figures(rate * 60_000.0)),
            last_progress_at: Some(Timestamp(last_at)),
            stalled_secs: stalled.then(|| round_secs(since)),
            typical_secs: history.map(|(median, ..)| round_secs(median)),
            runs: history.map(|(_, runs, ..)| runs),
            longer_than_usual: history.is_some_and(|(.., longer)| longer),
        })
    }
}

/// Many steady steps: high; some fairly steady: medium; else low.
fn confidence(gaps: usize, rates: &[f64]) -> EstimateConfidence {
    let mean = rates.iter().sum::<f64>() / rates.len() as f64;
    let variance = rates.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / rates.len() as f64;
    let spread = if mean > 0.0 {
        variance.sqrt() / mean
    } else {
        f64::INFINITY
    };
    if gaps >= 7 && spread < 0.35 {
        EstimateConfidence::High
    } else if gaps >= 3 && spread < 0.75 {
        EstimateConfidence::Medium
    } else {
        EstimateConfidence::Low
    }
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

/// Seconds, rounded so a display changes calmly: 5 s under a minute, 15 s
/// under ten minutes, then whole minutes.
fn round_secs(ms: i64) -> u32 {
    let secs = (ms.max(0) as f64 / 1_000.0).round();
    let step = if secs < 60.0 {
        5.0
    } else if secs < 600.0 {
        15.0
    } else {
        60.0
    };
    let rounded = ((secs / step).round() * step) as u64;
    u32::try_from(rounded).unwrap_or(u32::MAX)
}

/// Two significant figures: 12.0, 1.3, 0.44.
fn two_figures(value: f64) -> f32 {
    if value <= 0.0 || !value.is_finite() {
        return 0.0;
    }
    let digits = 1 - value.log10().floor() as i32;
    let scale = 10f64.powi(digits);
    ((value * scale).round() / scale) as f32
}
