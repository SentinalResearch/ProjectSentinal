//! `Server-Timing` on the control's dashboard reads, so Settings → Diagnostics
//! can show where a slow page load spent its time. Durations only: no names,
//! IDs, or counts that describe the owner's Machines.

use axum::{http::HeaderValue, response::Response};
use std::time::{Duration, Instant};

/// Named phases of one request, in order.
#[derive(Default)]
pub(super) struct ServerTiming {
    phases: Vec<(&'static str, Duration)>,
}

impl ServerTiming {
    /// Time `work` as phase `name`.
    pub(super) fn measure<T>(&mut self, name: &'static str, work: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let value = work();
        self.phases.push((name, started.elapsed()));
        value
    }

    /// Record an already-measured phase.
    pub(super) fn record(&mut self, name: &'static str, duration: Duration) {
        self.phases.push((name, duration));
    }

    /// Sum of all phases.
    pub(super) fn total(&self) -> Duration {
        self.phases.iter().map(|(_, duration)| *duration).sum()
    }

    fn header(&self) -> String {
        self.phases
            .iter()
            .map(|(name, duration)| format!("{name};dur={:.1}", duration.as_secs_f64() * 1_000.0))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Attach the header to `response`.
    pub(super) fn apply(&self, mut response: Response) -> Response {
        if let Ok(value) = HeaderValue::from_str(&self.header()) {
            response.headers_mut().insert("server-timing", value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::ServerTiming;
    use std::time::Duration;

    #[test]
    fn phases_are_listed_in_order_in_milliseconds() {
        let mut timing = ServerTiming::default();
        timing.record("auth", Duration::from_micros(310));
        assert_eq!(timing.measure("snapshot", || 7), 7);
        timing.record("tailscale", Duration::from_millis(41));
        let header = timing.header();
        assert!(
            header.starts_with("auth;dur=0.3, snapshot;dur=0.0"),
            "{header}"
        );
        assert!(header.ends_with(", tailscale;dur=41.0"), "{header}");
    }
}
