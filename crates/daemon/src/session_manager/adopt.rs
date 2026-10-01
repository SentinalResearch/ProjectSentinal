//! Read-only adoption of an existing same-user process (ADOPTED / PROCESS).
//!
//! Adoption only reads: it records the process identity and metadata, samples
//! Metrics through the process-tree root, and watches liveness. It never
//! signals, traces, reparents, reprioritizes, or opens the process's streams.

use super::{lock_session, tasks, SessionManager, SessionManagerError};
use crate::log_tail::LogTail;
use protocol::{
    CaptureLevel, DaemonMessage, LogSource, Session, SessionId, SessionLog, SessionOrigin,
    SessionState,
};
use std::{os::unix::fs::MetadataExt, path::Path};

const LOG_TAIL_READ_BYTES: usize = 64 * 1024;
const LOG_TAIL_POLL: std::time::Duration = std::time::Duration::from_millis(50);

impl SessionManager {
    /// Adopt `pid` read-only; `log_path` is recorded for 1b.D, not tailed.
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
        let tail = log.as_ref().map(open_tail).transpose()?;
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
        self.insert_external(session.clone())?;
        session = self.session(id)?;
        if let Some(tail) = tail {
            self.activate_log_tail(id, tail)?;
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
        let tail = match open_tail(&log) {
            Ok(tail) => Some(tail),
            Err(error) => {
                tracing::warn!(session = id.0, %error, "discovered log is not readable");
                None
            }
        };
        let handle = self.handle(id)?;
        let session = {
            let mut session = lock_session(&handle)?;
            if session.state.is_finished() || session.log.is_some() {
                return Ok(());
            }
            session.log = Some(log);
            if tail.is_some() {
                session.capture = CaptureLevel::ProcessLog;
            }
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
        if let Some(tail) = tail {
            self.spawn_log_tail(id, tail);
        }
        Ok(())
    }

    pub(super) fn activate_log_tail(
        &self,
        id: SessionId,
        tail: LogTail,
    ) -> Result<(), SessionManagerError> {
        let handle = self.handle(id)?;
        let session = {
            let mut session = lock_session(&handle)?;
            session.capture = CaptureLevel::ProcessLog;
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
        self.spawn_log_tail(id, tail);
        Ok(())
    }

    fn spawn_log_tail(&self, id: SessionId, mut tail: LogTail) {
        let manager = self.clone();
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                session = id.0,
                "cannot start adopted log tail without a Tokio runtime"
            );
            return;
        };
        runtime.spawn(async move {
            loop {
            let handle = match manager.handle(id) {
                Ok(handle) => handle,
                Err(error) => {
                    tracing::warn!(session = id.0, %error, "adopted log tail lost its Session");
                    return;
                }
            };
            match lock_session(&handle) {
                Ok(session) if session.state.is_finished() => return,
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(session = id.0, %error, "adopted log tail cannot inspect Session");
                    return;
                }
            }
            match tail.poll() {
                Ok(chunk) => {
                    if chunk.reset {
                        match handle.line_framer.lock() {
                            Ok(mut framer) => framer.reset(),
                            Err(_) => {
                                tracing::warn!(session = id.0, "adopted log line-framer lock poisoned");
                                return;
                            }
                        }
                    }
                    if !chunk.bytes.is_empty() {
                        tasks::apply_output(
                            id,
                            &handle,
                            manager.inner.store.as_deref(),
                            &chunk.bytes,
                            (manager.inner.config.clock)(),
                        );
                    }
                }
                Err(error) => {
                    tracing::debug!(session = id.0, %error, "adopted log temporarily unavailable");
                }
            }
                tokio::time::sleep(LOG_TAIL_POLL).await;
            }
        });
    }
}

pub(super) fn open_tail(log: &SessionLog) -> Result<LogTail, SessionManagerError> {
    LogTail::open(log.path.clone().into(), LOG_TAIL_READ_BYTES).map_err(|error| {
        SessionManagerError::InvalidLog {
            path: log.path.clone(),
            reason: error.to_string(),
        }
    })
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
