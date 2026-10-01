//! Deterministic Session health derived from structured observations.

use crate::ProtocolError;
use serde::{Deserialize, Serialize};

/// Maximum evidence entries retained with one Health snapshot.
pub const MAX_HEALTH_EVIDENCE: usize = 16;
/// Maximum UTF-8 bytes in one Health evidence entry.
pub const MAX_HEALTH_EVIDENCE_BYTES: usize = 512;

/// A common deterministic rating used by Health dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HealthLevel {
    /// There is not enough evidence for a rating.
    Unknown,
    /// Observations are within expected bounds.
    Good,
    /// Observations warrant attention.
    Warning,
    /// Observations indicate a serious problem.
    Critical,
}

/// Overall deterministic Session health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HealthOverall {
    /// The Session is progressing without warning evidence.
    Healthy,
    /// The Session has warning evidence but has not failed or stalled.
    Warning,
    /// The Session has been quiet with idle resources long enough to suspect a stall.
    PossiblyStalled,
    /// Error evidence or a failed Command indicates failure.
    Failed,
}

/// Current rule-based health assessment for one Session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Health {
    /// Current progress percentage in the inclusive range 0–100.
    pub progress: Option<f32>,
    /// Whether useful work is currently visible.
    pub activity: HealthLevel,
    /// Whether CPU/GPU/RAM observations are within expected bounds.
    pub resources: HealthLevel,
    /// Whether output and process behavior appear stable.
    pub stability: HealthLevel,
    /// Number of error observations in the active assessment.
    pub errors: u32,
    /// Overall deterministic result.
    pub overall: HealthOverall,
    /// Bounded human-readable evidence behind the ratings.
    pub evidence: Vec<String>,
}

impl Health {
    /// Validate progress and evidence bounds before persistence or transport.
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self
            .progress
            .is_some_and(|value| !value.is_finite() || !(0.0..=100.0).contains(&value))
        {
            return Err(ProtocolError::InvalidHealthProgress(
                self.progress.unwrap_or_default(),
            ));
        }
        if self.evidence.len() > MAX_HEALTH_EVIDENCE {
            return Err(ProtocolError::TooManyHealthEvidence {
                actual: self.evidence.len(),
                maximum: MAX_HEALTH_EVIDENCE,
            });
        }
        for value in &self.evidence {
            if value.is_empty() {
                return Err(ProtocolError::EmptyField {
                    field: "health evidence",
                });
            }
            if value.len() > MAX_HEALTH_EVIDENCE_BYTES {
                return Err(ProtocolError::FieldTooLong {
                    field: "health evidence",
                    actual: value.len(),
                    maximum: MAX_HEALTH_EVIDENCE_BYTES,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy() -> Health {
        Health {
            progress: Some(50.0),
            activity: HealthLevel::Good,
            resources: HealthLevel::Good,
            stability: HealthLevel::Good,
            errors: 0,
            overall: HealthOverall::Healthy,
            evidence: vec!["step 5 of 10".into()],
        }
    }

    #[test]
    fn valid_health_accepts_bounded_progress_and_evidence() {
        assert_eq!(healthy().validate(), Ok(()));
    }

    #[test]
    fn health_rejects_bad_progress_and_evidence() {
        let mut health = healthy();
        health.progress = Some(101.0);
        assert_eq!(
            health.validate(),
            Err(ProtocolError::InvalidHealthProgress(101.0))
        );

        health.progress = None;
        health.evidence = vec!["evidence".into(); MAX_HEALTH_EVIDENCE + 1];
        assert!(matches!(
            health.validate(),
            Err(ProtocolError::TooManyHealthEvidence { .. })
        ));
    }
}
