//! T0.11: a noisy build cannot flood the store with warning and error Events.
//!
//! The first [`BURST_KEEP`] warnings (or errors) of a burst are kept, so an
//! ordinary build's errors and its closing summary line still arrive. Further
//! ones within [`BURST_MS`] of the first are counted instead, and once the
//! window has passed a single Event says how many were left out ("15 more
//! warnings in 10 s"). Warnings and errors are counted separately.

use protocol::{EventKind, Timestamp};

/// How long one burst lasts after its first Event.
pub(crate) const BURST_MS: i64 = 10_000;
/// Events kept at the start of each burst.
pub(crate) const BURST_KEEP: u32 = 5;

#[derive(Debug, Default)]
struct Burst {
    started: Option<Timestamp>,
    kept: u32,
    left_out: u32,
}

impl Burst {
    /// Whether an Event at `at` is the first of a burst.
    fn admit(&mut self, at: Timestamp) -> bool {
        match self.started {
            Some(start) if at.0.saturating_sub(start.0) < BURST_MS => {
                if self.kept < BURST_KEEP {
                    self.kept += 1;
                    return true;
                }
                self.left_out = self.left_out.saturating_add(1);
                false
            }
            _ => {
                self.started = Some(at);
                self.kept = 1;
                self.left_out = 0;
                true
            }
        }
    }

    /// The count of a finished burst, once.
    fn finished(&mut self, at: Timestamp) -> Option<u32> {
        let start = self.started?;
        if at.0.saturating_sub(start.0) < BURST_MS || self.left_out == 0 {
            return None;
        }
        let count = self.left_out;
        *self = Burst::default();
        Some(count)
    }
}

/// Per-Session warning and error bursts.
#[derive(Debug, Default)]
pub(crate) struct Bursts {
    warnings: Burst,
    errors: Burst,
}

impl Bursts {
    /// Forget every burst, e.g. when a new Command starts.
    pub(crate) fn reset(&mut self) {
        *self = Bursts::default();
    }

    /// False for a warning or error past the first few of a running burst.
    pub(crate) fn admit(&mut self, at: Timestamp, event: &EventKind) -> bool {
        match event {
            EventKind::WarningDetected { .. } => self.warnings.admit(at),
            EventKind::ErrorDetected { .. } => self.errors.admit(at),
            _ => true,
        }
    }

    /// One Event per burst that ended by `at`, saying how many were left out.
    pub(crate) fn summaries(&mut self, at: Timestamp) -> Vec<EventKind> {
        let seconds = BURST_MS / 1_000;
        let mut events = Vec::new();
        if let Some(count) = self.warnings.finished(at) {
            let noun = if count == 1 { "warning" } else { "warnings" };
            events.push(EventKind::WarningDetected {
                message: format!("{count} more {noun} in {seconds} s"),
            });
        }
        if let Some(count) = self.errors.finished(at) {
            let noun = if count == 1 { "error" } else { "errors" };
            events.push(EventKind::ErrorDetected {
                message: format!("{count} more {noun} in {seconds} s"),
            });
        }
        events
    }
}
