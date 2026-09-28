//! OS process identity used to verify externally reported Session roots.
//!
//! A PID alone can be reused after its process exits, so an identity is the
//! PID's owner plus its OS-reported start time. A Session stays attached to a
//! PID only while both still match what was recorded at registration.

use protocol::Timestamp;
use std::sync::{Arc, Mutex};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// What the OS reports about one live process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessIdentity {
    /// Process start time (second precision on supported platforms).
    pub started_at: Timestamp,
    /// Owning user ID.
    pub uid: u32,
}

type IdentifyFn = dyn Fn(u32) -> Option<ProcessIdentity> + Send + Sync;

/// Looks up process identities for the daemon's own user.
#[derive(Clone)]
pub struct ProcessProbe {
    owner_uid: Option<u32>,
    identify: Arc<IdentifyFn>,
}

impl std::fmt::Debug for ProcessProbe {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProcessProbe")
            .field("owner_uid", &self.owner_uid)
            .finish_non_exhaustive()
    }
}

impl ProcessProbe {
    /// A probe for `owner_uid` using an injected lookup (tests, other OSes).
    pub fn new(
        owner_uid: u32,
        identify: impl Fn(u32) -> Option<ProcessIdentity> + Send + Sync + 'static,
    ) -> Self {
        ProcessProbe {
            owner_uid: Some(owner_uid),
            identify: Arc::new(identify),
        }
    }

    /// A probe that knows no processes, so external registration is refused.
    pub fn unavailable() -> Self {
        ProcessProbe {
            owner_uid: None,
            identify: Arc::new(|_| None),
        }
    }

    /// A probe backed by the operating system process table.
    pub fn system(owner_uid: u32) -> Self {
        let system = Mutex::new(System::new());
        Self::new(owner_uid, move |pid| {
            let mut system = system.lock().ok()?;
            let pid = Pid::from_u32(pid);
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing().with_user(UpdateKind::OnlyIfNotSet),
            );
            let process = system.process(pid)?;
            let seconds = i64::try_from(process.start_time()).ok()?;
            Some(ProcessIdentity {
                started_at: Timestamp(seconds.saturating_mul(1_000)),
                uid: **process.user_id()?,
            })
        })
    }

    /// The user whose processes may be registered, if the probe is usable.
    pub fn owner_uid(&self) -> Option<u32> {
        self.owner_uid
    }

    /// Current identity of `pid`, or `None` when it does not exist.
    pub fn identify(&self, pid: u32) -> Option<ProcessIdentity> {
        (self.identify)(pid)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_probe_has_no_owner_and_no_processes() {
        let probe = ProcessProbe::unavailable();
        assert_eq!(probe.owner_uid(), None);
        assert_eq!(probe.identify(std::process::id()), None);
    }

    #[test]
    fn system_probe_identifies_this_process_and_not_a_missing_one() {
        let probe = ProcessProbe::system(0);
        let identity = probe.identify(std::process::id()).unwrap();
        assert!(identity.started_at.0 > 0);
        assert_eq!(probe.identify(u32::MAX - 1), None);
    }
}
