//! Read-only adoption of an existing same-user process (ADOPTED / PROCESS).
//!
//! Adoption only reads: it records the process identity and metadata, samples
//! Metrics through the process-tree root, and watches liveness. It never
//! signals, traces, reparents, reprioritizes, or opens the process's streams.

use super::{adopted_log::open_tail, lock_session, SessionManager, SessionManagerError};
use protocol::{
    CaptureLevel, DaemonMessage, LogSource, Session, SessionId, SessionLog, SessionOrigin,
    SessionState,
};
use std::{os::unix::fs::MetadataExt, path::Path};

impl SessionManager {
    /// Adopt `pid` read-only, tailing `log_path` (or a discovered log) when usable.
    pub fn adopt_process(
        &self,
        pid: u32,
        title: Option<String>,
        log_path: Option<String>,
    ) -> Result<Session, SessionManagerError> {
        if pid == std::process::id() {
            return Err(SessionManagerError::AdoptSelf(pid));
        }
        let owner_uid = self
            .inner
            .probe
            .owner_uid()
            .ok_or(SessionManagerError::ProcessProbeUnavailable)?;
        let started_at = self.verify_owned_process(pid)?;
        let log = log_path
            .map(|path| explicit_log(&path, owner_uid))
            .transpose()?;
        let tail = log
            .as_ref()
            .map(|log| open_tail(log, owner_uid, None))
            .transpose()?;
        let _registration =
            self.inner
                .registration
                .lock()
                .map_err(|_| SessionManagerError::Lock {
                    resource: "adoption",
                    session_id: SessionId(0),
                })?;
        for handle in self.handles()? {
            let existing = lock_session(&handle)?.clone();
            if existing.origin != SessionOrigin::Adopted
                || existing.pid != Some(pid)
                || existing.state.is_finished()
            {
                continue;
            }
            if existing.process_started_at == Some(started_at) {
                return Err(SessionManagerError::AlreadyAdopted {
                    pid,
                    session_id: existing.id,
                });
            }
            // Same PID, different process: the old one is gone.
            self.end_external(&handle)?;
        }

        let details = self.inner.probe.details(pid);
        let id = SessionId(
            self.inner
                .next_id
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        );
        let now = (self.inner.config.clock)();
        let mut session = Session::new(
            id,
            self.inner.config.machine_id.clone(),
            details.exe.clone().unwrap_or_default(),
            details.cwd.clone().unwrap_or_default(),
            now,
        );
        session.origin = SessionOrigin::Adopted;
        session.capture = CaptureLevel::Process;
        session.title = title.or(details.name);
        session.pid = Some(pid);
        session.process_started_at = Some(started_at);
        session.log = log;
        session.transition(SessionState::Idle, now)?;
        session.transition(SessionState::Running, now)?;
        session.validate()?;
        if let Some(store) = &self.inner.store {
            store.insert_session(&session)?;
        }
        self.insert_external(session.clone(), details.command)?;
        session = self.session(id)?;
        if let (Some(tail), Some(log)) = (tail, &session.log) {
            self.activate_log_tail(id, tail, &log.clone())?;
            session = self.session(id)?;
        }
        self.observe_reduced(DaemonMessage::SessionUpdated {
            session: session.clone(),
        });
        if session.log.is_none() {
            self.spawn_log_discovery(id, pid);
        }
        Ok(session)
    }

    /// Look for a redirected stdout/stderr file without blocking the request.
    fn spawn_log_discovery(&self, id: SessionId, pid: u32) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let manager = self.clone();
        runtime.spawn_blocking(move || {
            let Some(log) = manager.inner.probe.discover_log(pid) else {
                return;
            };
            if let Err(error) = manager.record_discovered_log(id, log) {
                tracing::warn!(session = id.0, %error, "failed to record discovered log");
            }
        });
    }

    fn record_discovered_log(
        &self,
        id: SessionId,
        log: SessionLog,
    ) -> Result<(), SessionManagerError> {
        let handle = self.handle(id)?;
        let session = {
            let mut session = lock_session(&handle)?;
            if session.state.is_finished() || session.log.is_some() {
                return Ok(());
            }
            session.log = Some(log.clone());
            session.validate()?;
            session.clone()
        };
        if let Some(store) = &self.inner.store {
            store.update_session(&session)?;
        }
        self.observe_reduced(DaemonMessage::SessionUpdated {
            session: session.clone(),
        });
        let _ = handle
            .structured
            .send(DaemonMessage::SessionUpdated { session });
        match self.open_session_tail(id, &log) {
            Ok(tail) => self.activate_log_tail(id, tail, &log),
            Err(error) => self.warn_log_unavailable(id, &log, &error),
        }
    }
}

/// Accept an explicit log only as an absolute, readable regular file.
fn explicit_log(path: &str, owner_uid: u32) -> Result<SessionLog, SessionManagerError> {
    let invalid = |reason: &str| SessionManagerError::InvalidLog {
        path: path.to_string(),
        reason: reason.to_string(),
    };
    if !path.starts_with('/') {
        return Err(invalid("must be an absolute path"));
    }
    let metadata = std::fs::metadata(Path::new(path))
        .map_err(|error| invalid(&format!("cannot read: {error}")))?;
    if !metadata.is_file() {
        return Err(invalid("is not a regular file"));
    }
    if metadata.uid() != owner_uid {
        return Err(invalid(&format!(
            "log uid {} does not match daemon uid {owner_uid}",
            metadata.uid()
        )));
    }
    std::fs::File::open(path).map_err(|error| invalid(&format!("cannot read: {error}")))?;
    Ok(SessionLog {
        path: path.to_string(),
        source: LogSource::Explicit,
    })
}
