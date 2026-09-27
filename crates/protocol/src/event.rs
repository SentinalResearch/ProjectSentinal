//! Events: the common language of the system.
//!
//! Raw observations (bytes, exit codes, CPU samples) are turned into events by
//! the daemon and watchers. Everything above that layer — UI, notifications,
//! the intelligence layer — reads events, never raw terminal output.
//!
//! On the wire an event kind looks like:
//! `{"type": "PROGRESS_UPDATED", "current": 37, "total": 100, "unit": "epoch"}`

use crate::{CommandId, DurationMs, EventId, SessionId, Timestamp};
use serde::{Deserialize, Serialize};

/// A resource that can run high.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Resource {
    /// CPU usage.
    Cpu,
    /// System memory.
    Ram,
    /// GPU compute.
    Gpu,
    /// GPU memory.
    Vram,
    /// Disk space.
    Disk,
}

/// How important an event is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Severity {
    /// Normal progress.
    Info,
    /// Worth a look.
    Warning,
    /// Something broke.
    Error,
}

/// What happened. Each variant carries only the data specific to it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
#[serde(tag = "type", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EventKind {
    /// A command began.
    CommandStarted {
        /// The exact command text.
        text: String,
    },
    /// A command ended.
    CommandFinished {
        /// Its exit code.
        exit_code: i32,
        /// How long it ran.
        duration_ms: DurationMs,
    },
    /// A process appeared in the session's process tree.
    ProcessStarted {
        /// OS process ID.
        pid: u32,
        /// Executable name.
        name: String,
    },
    /// A process left the session's process tree.
    ProcessExited {
        /// OS process ID.
        pid: u32,
        /// Exit code if known.
        exit_code: Option<i32>,
    },
    /// Measurable progress, e.g. epoch 37 of 100.
    ProgressUpdated {
        /// Steps done so far.
        current: u64,
        /// Total steps, if known.
        total: Option<u64>,
        /// What a step is, e.g. `"epoch"`, `"test"`, `"%"`.
        unit: String,
    },
    /// A warning line was recognised in the output.
    WarningDetected {
        /// Short description.
        message: String,
    },
    /// An error was recognised in the output.
    ErrorDetected {
        /// Short description.
        message: String,
    },
    /// A resource crossed its high threshold.
    ResourceHigh {
        /// Which resource.
        resource: Resource,
        /// Usage as a percentage, 0–100.
        percent: f32,
    },
    /// The running command seems to be waiting for the user.
    WaitingForInput {
        /// The prompt text, if recognised, e.g. `"Continue? [y/N]"`.
        prompt: Option<String>,
    },
    /// No output and no resource activity for a while.
    PossiblyStalled {
        /// How long it has been quiet.
        silent_for_ms: DurationMs,
    },
    /// A checkpoint/model file was written.
    CheckpointCreated {
        /// Path of the checkpoint.
        path: String,
    },
    /// A server started listening.
    ServerStarted {
        /// Port, if known.
        port: Option<u16>,
        /// Full URL, if known.
        url: Option<String>,
    },
}

impl EventKind {
    /// How important this event is.
    pub fn severity(&self) -> Severity {
        match self {
            EventKind::CommandFinished { exit_code, .. } if *exit_code != 0 => Severity::Error,
            EventKind::ErrorDetected { .. } => Severity::Error,
            EventKind::WarningDetected { .. }
            | EventKind::ResourceHigh { .. }
            | EventKind::WaitingForInput { .. }
            | EventKind::PossiblyStalled { .. } => Severity::Warning,
            _ => Severity::Info,
        }
    }

    /// Whether this event sends a notification under the default rules.
    ///
    /// Defaults from the spec: finished, failed, waiting for input, stalled and
    /// serious resource issues notify; start and normal progress stay silent.
    /// Sessions can override this later.
    pub fn notifies_by_default(&self) -> bool {
        match self {
            EventKind::CommandFinished { .. }
            | EventKind::ErrorDetected { .. }
            | EventKind::WaitingForInput { .. }
            | EventKind::PossiblyStalled { .. } => true,
            EventKind::ResourceHigh { percent, .. } => *percent >= 95.0,
            _ => false,
        }
    }

    /// Progress as a percentage (0–100), if `total` is known and non-zero.
    pub fn progress_percent(&self) -> Option<f32> {
        match self {
            EventKind::ProgressUpdated {
                current,
                total: Some(total),
                ..
            } if *total > 0 => {
                let pct = (*current as f64 / *total as f64 * 100.0).min(100.0);
                Some(pct as f32)
            }
            _ => None,
        }
    }
}

/// Something that happened in a session, at a point in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts", ts(export))]
pub struct Event {
    /// ID, increasing over time on this machine.
    pub id: EventId,
    /// Session it happened in.
    pub session_id: SessionId,
    /// Command it relates to, if any.
    pub command_id: Option<CommandId>,
    /// When it happened.
    pub at: Timestamp,
    /// What happened.
    pub kind: EventKind,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(current: u64, total: Option<u64>) -> EventKind {
        EventKind::ProgressUpdated {
            current,
            total,
            unit: "epoch".into(),
        }
    }

    #[test]
    fn progress_percent_is_computed_from_current_and_total() {
        assert_eq!(progress(37, Some(100)).progress_percent(), Some(37.0));
        assert_eq!(progress(1, Some(4)).progress_percent(), Some(25.0));
    }

    #[test]
    fn progress_percent_is_none_when_total_unknown_or_zero() {
        assert_eq!(progress(37, None).progress_percent(), None);
        assert_eq!(progress(37, Some(0)).progress_percent(), None);
    }

    #[test]
    fn progress_percent_is_capped_at_100() {
        assert_eq!(progress(150, Some(100)).progress_percent(), Some(100.0));
    }

    #[test]
    fn progress_percent_is_none_for_other_kinds() {
        let kind = EventKind::WarningDetected {
            message: "x".into(),
        };
        assert_eq!(kind.progress_percent(), None);
    }

    #[test]
    fn failed_command_is_an_error_but_successful_one_is_info() {
        let ok = EventKind::CommandFinished {
            exit_code: 0,
            duration_ms: DurationMs(1),
        };
        let failed = EventKind::CommandFinished {
            exit_code: 1,
            duration_ms: DurationMs(1),
        };
        assert_eq!(ok.severity(), Severity::Info);
        assert_eq!(failed.severity(), Severity::Error);
    }

    #[test]
    fn start_and_progress_are_silent_by_default() {
        let started = EventKind::CommandStarted { text: "ls".into() };
        assert!(!started.notifies_by_default());
        assert!(!progress(10, Some(100)).notifies_by_default());
    }

    #[test]
    fn finished_waiting_stalled_and_errors_notify_by_default() {
        let kinds = [
            EventKind::CommandFinished {
                exit_code: 0,
                duration_ms: DurationMs(1),
            },
            EventKind::WaitingForInput { prompt: None },
            EventKind::PossiblyStalled {
                silent_for_ms: DurationMs(1),
            },
            EventKind::ErrorDetected {
                message: "CUDA OOM".into(),
            },
        ];
        for kind in kinds {
            assert!(kind.notifies_by_default(), "{kind:?} should notify");
        }
    }

    #[test]
    fn resource_high_notifies_only_when_serious() {
        let high = |percent| EventKind::ResourceHigh {
            resource: Resource::Vram,
            percent,
        };
        assert!(!high(90.0).notifies_by_default());
        assert!(high(95.0).notifies_by_default());
    }

    #[test]
    fn event_kind_is_tagged_with_type_on_the_wire() {
        let json = serde_json::to_value(progress(37, Some(100))).unwrap();
        assert_eq!(json["type"], "PROGRESS_UPDATED");
        assert_eq!(json["current"], 37);
    }
}
