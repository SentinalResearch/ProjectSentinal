//! A bound on one Session's saved output (F9). Live output keeps streaming to
//! attached clients; only the compressed log on disk stops growing, with one
//! visible marker so a replay says why the tail is missing.

/// Uncompressed bytes saved per Session before its log stops growing.
pub(super) const SESSION_LOG_CAP_BYTES: u64 = 1024 * 1024 * 1024;

/// Appended once, when a Session's log reaches its cap.
pub(super) const CAP_MARKER: &[u8] =
    b"\r\n[sentinal: saved output stopped at the 1 GiB limit; live output continues]\r\n";

/// What to do with the next chunk of output.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Admit {
    /// Save the chunk.
    Write,
    /// Save the cap marker instead, then nothing more.
    Marker,
    /// The log is full; save nothing.
    Skip,
}

/// Running total for one Session's log.
#[derive(Debug)]
pub(super) struct LogBudget {
    cap: u64,
    used: u64,
    full: bool,
}

impl LogBudget {
    pub(super) fn new(cap: u64) -> Self {
        Self {
            cap,
            used: 0,
            full: false,
        }
    }

    /// Decide whether `len` more bytes may be saved.
    pub(super) fn admit(&mut self, len: usize) -> Admit {
        if self.full {
            return Admit::Skip;
        }
        let next = self.used.saturating_add(len as u64);
        if next > self.cap {
            self.full = true;
            return Admit::Marker;
        }
        self.used = next;
        Admit::Write
    }
}

#[cfg(test)]
mod tests {
    use super::{Admit, LogBudget};

    #[test]
    fn a_session_log_stops_once_at_its_cap_with_one_marker() {
        let mut budget = LogBudget::new(10);
        assert_eq!(budget.admit(6), Admit::Write);
        assert_eq!(budget.admit(4), Admit::Write);
        assert_eq!(budget.admit(1), Admit::Marker);
        assert_eq!(budget.admit(1), Admit::Skip);
        assert_eq!(budget.admit(0), Admit::Skip);
    }
}
