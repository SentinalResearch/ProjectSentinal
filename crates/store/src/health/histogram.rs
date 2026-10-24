//! A mergeable fixed-bucket histogram: four buckets per doubling.

use protocol::observability::HealthStat;
use std::collections::BTreeMap;

/// Histogram bucket of `value`: four buckets per doubling (about ±9 %).
pub fn bucket(value: f64) -> u32 {
    if value <= 0.0 || !value.is_finite() {
        return 0;
    }
    // Truncation is the bucketing; values are bounded by `max_value`.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let index = (4.0 * (value + 1.0).log2()).floor() as u32;
    index
}

/// The middle (geometric) of a bucket.
fn bucket_value(index: u32) -> f64 {
    let low = 2f64.powf(f64::from(index) / 4.0) - 1.0;
    let high = 2f64.powf(f64::from(index + 1) / 4.0) - 1.0;
    ((low + 1.0) * (high + 1.0)).sqrt() - 1.0
}

/// A mergeable summary of many samples.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Histogram {
    pub(super) count: u32,
    pub(super) sum: f64,
    pub(super) min: f64,
    pub(super) max: f64,
    buckets: BTreeMap<u32, u32>,
}

impl Histogram {
    /// Add one value.
    pub fn add(&mut self, value: f64) {
        if self.count == 0 {
            self.min = value;
            self.max = value;
        } else {
            self.min = self.min.min(value);
            self.max = self.max.max(value);
        }
        self.count += 1;
        self.sum += value;
        *self.buckets.entry(bucket(value)).or_default() += 1;
    }

    /// Fold another histogram in.
    pub fn merge(&mut self, other: &Histogram) {
        if other.count == 0 {
            return;
        }
        if self.count == 0 {
            *self = other.clone();
            return;
        }
        self.count += other.count;
        self.sum += other.sum;
        self.min = self.min.min(other.min);
        self.max = self.max.max(other.max);
        for (index, count) in &other.buckets {
            *self.buckets.entry(*index).or_default() += count;
        }
    }

    fn quantile(&self, q: f64) -> f64 {
        // Rank of the q-th sample, 1-based.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rank = ((q * f64::from(self.count)).ceil() as u32).max(1);
        let mut seen = 0;
        for (index, count) in &self.buckets {
            seen += count;
            if seen >= rank {
                return bucket_value(*index).clamp(self.min, self.max);
            }
        }
        self.max
    }

    /// The summary, or `None` when empty.
    pub fn stat(&self) -> Option<HealthStat> {
        (self.count > 0).then(|| HealthStat {
            count: self.count,
            sum: self.sum,
            min: self.min,
            p50: self.quantile(0.5),
            p95: self.quantile(0.95),
            max: self.max,
        })
    }

    pub(super) fn encode(&self) -> String {
        serde_json::to_string(&self.buckets).unwrap_or_else(|_| "{}".into())
    }

    pub(super) fn from_row(count: u32, sum: f64, min: f64, max: f64, buckets: &str) -> Self {
        Histogram {
            count,
            sum,
            min,
            max,
            buckets: serde_json::from_str(buckets).unwrap_or_default(),
        }
    }
}
