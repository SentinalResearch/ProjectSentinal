//! Time on the wire: Unix milliseconds everywhere.
//!
//! The protocol never reads the clock itself. Callers pass timestamps in,
//! which keeps every function here deterministic and easy to test.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A point in time, as milliseconds since the Unix epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(transparent)]
pub struct Timestamp(pub i64);

/// A length of time in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(transparent)]
pub struct DurationMs(pub u64);

impl Timestamp {
    /// Time elapsed from `earlier` to `self`. Returns zero if `earlier` is later.
    pub fn since(self, earlier: Timestamp) -> DurationMs {
        let diff = self.0.saturating_sub(earlier.0);
        DurationMs(diff.max(0) as u64)
    }
}

impl DurationMs {
    /// Build a duration from whole seconds.
    pub fn from_secs(secs: u64) -> Self {
        DurationMs(secs.saturating_mul(1000))
    }

    /// Whole seconds, rounded down.
    pub fn as_secs(self) -> u64 {
        self.0 / 1000
    }
}

impl fmt::Display for DurationMs {
    /// Short human form, matching the UI: `850ms`, `42s`, `3m05s`, `1h31m`, `2d4h`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ms = self.0;
        let secs = ms / 1000;
        let (mins, hours, days) = (secs / 60, secs / 3600, secs / 86_400);
        if ms < 1000 {
            write!(f, "{ms}ms")
        } else if secs < 60 {
            write!(f, "{secs}s")
        } else if mins < 60 {
            write!(f, "{mins}m{:02}s", secs % 60)
        } else if hours < 24 {
            write!(f, "{hours}h{:02}m", mins % 60)
        } else {
            write!(f, "{days}d{}h", hours % 24)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_returns_elapsed_time() {
        assert_eq!(Timestamp(5_000).since(Timestamp(2_000)), DurationMs(3_000));
    }

    #[test]
    fn since_is_zero_when_other_time_is_later() {
        assert_eq!(Timestamp(1_000).since(Timestamp(9_000)), DurationMs(0));
    }

    #[test]
    fn duration_displays_in_short_human_form() {
        let cases = [
            (0, "0ms"),
            (850, "850ms"),
            (999, "999ms"),
            (1_000, "1s"),
            (42_000, "42s"),
            (59_999, "59s"),
            (60_000, "1m00s"),
            (185_000, "3m05s"),
            (3_600_000, "1h00m"),
            (5_460_000, "1h31m"),
            (86_400_000, "1d0h"),
            (187_200_000, "2d4h"),
        ];
        for (ms, expected) in cases {
            assert_eq!(DurationMs(ms).to_string(), expected, "for {ms} ms");
        }
    }

    #[test]
    fn from_secs_and_as_secs_round_trip() {
        assert_eq!(DurationMs::from_secs(42).as_secs(), 42);
        assert_eq!(DurationMs(1_999).as_secs(), 1);
    }
}
