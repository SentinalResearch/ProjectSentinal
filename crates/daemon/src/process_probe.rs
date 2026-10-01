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

/// Optional descriptive metadata; any field may be unavailable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessDetails {
    /// Short process name.
    pub name: Option<String>,
    /// Executable path.
    pub exe: Option<String>,
    /// Current working directory.
    pub cwd: Option<String>,
    /// Argument vector, used in memory only to select a watcher adapter.
    ///
    /// It can contain secrets, so it is never persisted or sent to clients.
    pub command: Option<Vec<String>>,
}

type IdentifyFn = dyn Fn(u32) -> Option<ProcessIdentity> + Send + Sync;
type DetailsFn = dyn Fn(u32) -> ProcessDetails + Send + Sync;
type DiscoverFn = dyn Fn(u32) -> Option<protocol::SessionLog> + Send + Sync;

/// Looks up process identities for the daemon's own user.
///
/// Every operation is a read. There is deliberately no signal, write, debug,
/// priority, or kill method: adopted processes are observed, never controlled.
#[derive(Clone)]
pub struct ProcessProbe {
    owner_uid: Option<u32>,
    identify: Arc<IdentifyFn>,
    details: Arc<DetailsFn>,
    discover_log: Arc<DiscoverFn>,
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
            details: Arc::new(|_| ProcessDetails::default()),
            discover_log: Arc::new(|_| None),
        }
    }

    /// Use `details` to describe processes (name, executable, cwd).
    pub fn with_details(
        mut self,
        details: impl Fn(u32) -> ProcessDetails + Send + Sync + 'static,
    ) -> Self {
        self.details = Arc::new(details);
        self
    }

    /// Use `discover` to find a process's redirected stdout/stderr log file.
    pub fn with_log_discovery(
        mut self,
        discover: impl Fn(u32) -> Option<protocol::SessionLog> + Send + Sync + 'static,
    ) -> Self {
        self.discover_log = Arc::new(discover);
        self
    }

    /// A probe that knows no processes, so external registration is refused.
    pub fn unavailable() -> Self {
        ProcessProbe {
            owner_uid: None,
            identify: Arc::new(|_| None),
            details: Arc::new(|_| ProcessDetails::default()),
            discover_log: Arc::new(|_| None),
        }
    }

    /// A probe backed by the operating system process table.
    pub fn system(owner_uid: u32) -> Self {
        let system = Arc::new(Mutex::new(System::new()));
        let for_details = system.clone();
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
        .with_details(move |pid| {
            let Ok(mut system) = for_details.lock() else {
                return ProcessDetails::default();
            };
            let pid = Pid::from_u32(pid);
            system.refresh_processes_specifics(
                ProcessesToUpdate::Some(&[pid]),
                true,
                ProcessRefreshKind::nothing()
                    .with_exe(UpdateKind::OnlyIfNotSet)
                    .with_cwd(UpdateKind::Always)
                    .with_cmd(UpdateKind::OnlyIfNotSet),
            );
            let Some(process) = system.process(pid) else {
                return ProcessDetails::default();
            };
            let path = |path: Option<&std::path::Path>| {
                path.map(|path| path.to_string_lossy().into_owned())
                    .filter(|path| !path.is_empty())
            };
            ProcessDetails {
                name: Some(process.name().to_string_lossy().into_owned()).filter(|n| !n.is_empty()),
                exe: path(process.exe()),
                cwd: path(process.cwd()),
                command: Some(
                    process
                        .cmd()
                        .iter()
                        .map(|argument| argument.to_string_lossy().into_owned())
                        .collect::<Vec<_>>(),
                )
                .filter(|argv| !argv.is_empty()),
            }
        })
        .with_log_discovery(crate::log_discovery::discover)
    }

    /// The user whose processes may be registered, if the probe is usable.
    pub fn owner_uid(&self) -> Option<u32> {
        self.owner_uid
    }

    /// Current identity of `pid`, or `None` when it does not exist.
    pub fn identify(&self, pid: u32) -> Option<ProcessIdentity> {
        (self.identify)(pid)
    }

    /// Descriptive metadata for `pid`; missing fields are not errors.
    pub fn details(&self, pid: u32) -> ProcessDetails {
        (self.details)(pid)
    }

    /// A regular file receiving `pid`'s stdout/stderr, if one can be found.
    /// May be slow (it can run `lsof`); never call it on a prompt or CLI path.
    pub fn discover_log(&self, pid: u32) -> Option<protocol::SessionLog> {
        (self.discover_log)(pid)
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
    fn system_probe_reports_this_process_argv() {
        let probe = ProcessProbe::system(0);
        let argv = probe.details(std::process::id()).command.unwrap();
        assert!(!argv.is_empty());
    }

    #[test]
    fn system_probe_identifies_this_process_and_not_a_missing_one() {
        let probe = ProcessProbe::system(0);
        let identity = probe.identify(std::process::id()).unwrap();
        assert!(identity.started_at.0 > 0);
        assert_eq!(probe.identify(u32::MAX - 1), None);
    }
}
