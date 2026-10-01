//! Which Events and Health claims each capture level can support.
//!
//! A reduced-capture Session must not claim what Sentinal could not observe:
//! no output means no stall/progress/error claims, and an adopted process has
//! no exit code, so neither its Events nor its Health may say it failed.

use crate::{CaptureLevel, EventKind};

impl CaptureLevel {
    /// Whether this capture observes enough to support `kind` as evidence.
    pub fn supports_evidence(self, kind: &EventKind) -> bool {
        let output = matches!(
            kind,
            EventKind::ProgressUpdated { .. }
                | EventKind::WarningDetected { .. }
                | EventKind::ErrorDetected { .. }
                | EventKind::WaitingForInput { .. }
                | EventKind::PossiblyStalled { .. }
                | EventKind::CheckpointCreated { .. }
                | EventKind::ServerStarted { .. }
        );
        match kind {
            EventKind::CommandStarted { .. } | EventKind::CommandFinished { .. } => {
                self.capabilities().exposes_commands
            }
            // Only a daemon-owned PTY knows a child's exit status.
            EventKind::ProcessExited {
                exit_code: Some(_), ..
            } => self == CaptureLevel::Full,
            EventKind::ProcessStarted { .. }
            | EventKind::ProcessExited {
                exit_code: None, ..
            }
            | EventKind::ResourceHigh { .. } => true,
            _ if output => {
                let capabilities = self.capabilities();
                capabilities.exposes_terminal_output || capabilities.exposes_log_output
            }
            _ => false,
        }
    }

    /// Whether `kind` sends a notification for a Session with this capture.
    pub fn notifies(self, kind: &EventKind) -> bool {
        self.supports_evidence(kind) && kind.notifies_by_default()
    }

    /// Whether this capture can know that its Session failed.
    ///
    /// FULL has the shell's exit status and COMMANDS has each Command's exit
    /// code; PROCESS_LOG and PROCESS have neither, so error lines stay warnings.
    pub const fn supports_failure_evidence(self) -> bool {
        matches!(self, CaptureLevel::Full | CaptureLevel::Commands)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DurationMs, Resource};

    fn output_kinds() -> Vec<EventKind> {
        vec![
            EventKind::ProgressUpdated {
                current: 1,
                total: Some(10),
                unit: "epoch".into(),
            },
            EventKind::WarningDetected {
                message: "w".into(),
            },
            EventKind::ErrorDetected {
                message: "e".into(),
            },
            EventKind::WaitingForInput { prompt: None },
            EventKind::PossiblyStalled {
                silent_for_ms: DurationMs(1),
            },
            EventKind::CheckpointCreated { path: "/c".into() },
            EventKind::ServerStarted {
                port: Some(8000),
                url: None,
            },
        ]
    }

    #[test]
    fn full_supports_every_kind() {
        for kind in output_kinds() {
            assert!(CaptureLevel::Full.supports_evidence(&kind), "{kind:?}");
        }
        assert!(
            CaptureLevel::Full.supports_evidence(&EventKind::ProcessExited {
                pid: 1,
                exit_code: Some(1)
            })
        );
    }

    #[test]
    fn process_log_supports_log_derived_evidence_but_no_commands() {
        for kind in output_kinds() {
            assert!(
                CaptureLevel::ProcessLog.supports_evidence(&kind),
                "{kind:?}"
            );
        }
        assert!(
            !CaptureLevel::ProcessLog.supports_evidence(&EventKind::CommandFinished {
                exit_code: 1,
                duration_ms: DurationMs(1)
            })
        );
    }

    #[test]
    fn commands_and_process_support_no_output_derived_evidence() {
        for capture in [CaptureLevel::Commands, CaptureLevel::Process] {
            for kind in output_kinds() {
                assert!(!capture.supports_evidence(&kind), "{capture} {kind:?}");
            }
        }
    }

    #[test]
    fn reduced_capture_never_claims_a_known_exit_code() {
        for capture in [
            CaptureLevel::Commands,
            CaptureLevel::ProcessLog,
            CaptureLevel::Process,
        ] {
            assert!(!capture.supports_evidence(&EventKind::ProcessExited {
                pid: 1,
                exit_code: Some(0)
            }));
            assert!(capture.supports_evidence(&EventKind::ProcessExited {
                pid: 1,
                exit_code: None
            }));
        }
    }

    #[test]
    fn process_notifies_only_for_ending_and_serious_resource_use() {
        let ended = EventKind::ProcessExited {
            pid: 1,
            exit_code: None,
        };
        let resource = EventKind::ResourceHigh {
            resource: Resource::Ram,
            percent: 97.0,
        };
        let stalled = EventKind::PossiblyStalled {
            silent_for_ms: DurationMs(600_000),
        };
        assert!(CaptureLevel::Process.supports_evidence(&ended));
        assert!(CaptureLevel::Process.notifies(&resource));
        assert!(!CaptureLevel::Process.notifies(&stalled));
        assert!(CaptureLevel::Full.notifies(&stalled));
    }

    #[test]
    fn only_full_and_commands_can_know_failure() {
        assert!(CaptureLevel::Full.supports_failure_evidence());
        assert!(CaptureLevel::Commands.supports_failure_evidence());
        assert!(!CaptureLevel::ProcessLog.supports_failure_evidence());
        assert!(!CaptureLevel::Process.supports_failure_evidence());
    }
}
