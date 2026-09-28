//! Ownership and lifecycle management for persistent terminal sessions.

mod capture;
mod error;
mod external;
mod external_lifecycle;
mod lifecycle;
mod structured;
mod tasks;

use crate::{
    audit::{AuditLog, AuditRecord},
    process_probe::ProcessProbe,
    pty::{PtyBackend, PtyError},
    scrollback::Scrollback,
    tracker::{CommandTracker, TrackerIds},
};
use capture::{require_capture, CaptureOperation};
pub use error::SessionManagerError;
use protocol::{
    DaemonMessage, MachineId, Session, SessionId, SessionOrigin, SessionState, Timestamp,
};
use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    sync::{atomic::AtomicU64, Arc, Mutex, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};
use store::Store;
use tokio::sync::broadcast;

/// Function used to timestamp session and audit state.
pub type Clock = Arc<dyn Fn() -> Timestamp + Send + Sync>;

/// Settings shared by every session in a manager.
#[derive(Clone)]
pub struct SessionManagerConfig {
    /// Machine that owns these sessions.
    pub machine_id: MachineId,
    /// Root directory for persistent data.
    pub data_dir: PathBuf,
    /// Number of terminal lines retained per session.
    pub scrollback_lines: usize,
    /// Time source, injectable for deterministic tests.
    pub clock: Clock,
}

impl SessionManagerConfig {
    /// Build production settings with the system clock.
    pub fn system(machine_id: MachineId, data_dir: PathBuf, scrollback_lines: usize) -> Self {
        SessionManagerConfig {
            machine_id,
            data_dir,
            scrollback_lines,
            clock: Arc::new(system_timestamp),
        }
    }
}

/// Values needed to create one session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateSession {
    /// Shell executable, or `None` for `$SHELL`/`/bin/sh`.
    pub shell: Option<String>,
    /// Initial working directory, or `None` for the user's home.
    pub cwd: Option<PathBuf>,
    /// Initial terminal width.
    pub cols: u16,
    /// Initial terminal height.
    pub rows: u16,
    /// Approved daemon-owned Session origin.
    pub origin: SessionOrigin,
    /// Optional user-visible title.
    pub title: Option<String>,
    /// Optional initial argv for WATCH Sessions, typed once after shell readiness.
    pub command: Option<Vec<String>>,
}

/// A screen snapshot followed by a receiver for live terminal bytes.
pub struct Attachment {
    /// ANSI bytes sufficient to redraw the current screen.
    pub screen: Vec<u8>,
    /// Live output produced after attachment.
    pub output: broadcast::Receiver<Vec<u8>>,
    /// Structured command, event, Session, and metric updates.
    pub structured: broadcast::Receiver<DaemonMessage>,
}

struct SessionHandle {
    id: SessionId,
    root_pid: Option<u32>,
    session: Mutex<Session>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    resize: Arc<dyn Fn(u16, u16) -> Result<(), PtyError> + Send + Sync>,
    kill: Arc<dyn Fn() -> Result<(), PtyError> + Send + Sync>,
    parser: Arc<Mutex<vt100::Parser>>,
    scrollback: Arc<Mutex<Scrollback>>,
    output: broadcast::Sender<Vec<u8>>,
    structured: broadcast::Sender<DaemonMessage>,
    tracker: Mutex<CommandTracker>,
    /// Rendered Watch initial Command, taken and typed at the first prompt.
    initial_command: Mutex<Option<Vec<u8>>>,
    /// Sequence bookkeeping for Commands reported by an external shell hook.
    shell_reports: Mutex<external::ShellReports>,
}

struct SessionManagerInner {
    backend: Arc<dyn PtyBackend>,
    config: SessionManagerConfig,
    audit: AuditLog,
    sessions: RwLock<HashMap<SessionId, Arc<SessionHandle>>>,
    next_id: AtomicU64,
    tracker_ids: TrackerIds,
    store: Option<Arc<Store>>,
    probe: ProcessProbe,
    /// Serializes external shell registration so concurrent reports create one Session.
    registration: Mutex<()>,
}

/// Owner of all persistent sessions on one daemon.
#[derive(Clone)]
pub struct SessionManager {
    inner: Arc<SessionManagerInner>,
}

impl SessionManager {
    /// Create an empty session manager using the selected PTY backend.
    pub fn new(
        backend: Arc<dyn PtyBackend>,
        config: SessionManagerConfig,
        audit: AuditLog,
    ) -> Self {
        Self::build(backend, config, audit, None, 1, 1, 1)
    }

    /// Create a manager backed by persistent structured storage.
    pub fn with_store(
        backend: Arc<dyn PtyBackend>,
        config: SessionManagerConfig,
        audit: AuditLog,
        store: Arc<Store>,
    ) -> Result<Self, SessionManagerError> {
        let next_session = store.next_session_id()?;
        let next_command = store.next_command_id()?;
        let next_event = store.next_event_id()?;
        Ok(Self::build(
            backend,
            config,
            audit,
            Some(store),
            next_session,
            next_command,
            next_event,
        ))
    }

    fn build(
        backend: Arc<dyn PtyBackend>,
        config: SessionManagerConfig,
        audit: AuditLog,
        store: Option<Arc<Store>>,
        next_session: u64,
        next_command: u64,
        next_event: u64,
    ) -> Self {
        SessionManager {
            inner: Arc::new(SessionManagerInner {
                backend,
                config,
                audit,
                sessions: RwLock::new(HashMap::new()),
                next_id: AtomicU64::new(next_session),
                tracker_ids: TrackerIds::starting_at(next_command, next_event),
                store,
                probe: ProcessProbe::unavailable(),
                registration: Mutex::new(()),
            }),
        }
    }

    /// Return all sessions ordered by ID.
    pub fn list(&self) -> Result<Vec<Session>, SessionManagerError> {
        let sessions = self
            .inner
            .sessions
            .read()
            .map_err(|_| SessionManagerError::Lock {
                resource: "session map",
                session_id: SessionId(0),
            })?;
        let mut result = sessions
            .values()
            .map(|handle| lock_session(handle).map(|session| session.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        result.sort_by_key(|session| session.id);
        Ok(result)
    }

    /// Return one session's current structured state.
    pub fn session(&self, session_id: SessionId) -> Result<Session, SessionManagerError> {
        let handle = self.handle(session_id)?;
        let session = lock_session(&handle)?.clone();
        Ok(session)
    }

    /// Snapshot the current screen and subscribe to subsequent output.
    pub fn attach(&self, session_id: SessionId) -> Result<Attachment, SessionManagerError> {
        let handle = self.handle(session_id)?;
        require_capture(&handle, CaptureOperation::Attach)?;
        let screen = handle
            .parser
            .lock()
            .map_err(|_| SessionManagerError::Lock {
                resource: "screen",
                session_id,
            })?
            .screen()
            .state_formatted();
        Ok(Attachment {
            screen,
            output: handle.output.subscribe(),
            structured: handle.structured.subscribe(),
        })
    }

    /// Return the retained line-oriented scrollback.
    pub fn scrollback(&self, session_id: SessionId) -> Result<Vec<String>, SessionManagerError> {
        let handle = self.handle(session_id)?;
        let lines = handle
            .scrollback
            .lock()
            .map_err(|_| SessionManagerError::Lock {
                resource: "scrollback",
                session_id,
            })?
            .lines();
        Ok(lines)
    }

    /// Resize a session's PTY and in-memory screen.
    pub fn resize(
        &self,
        session_id: SessionId,
        cols: u16,
        rows: u16,
    ) -> Result<(), SessionManagerError> {
        validate_size(cols, rows)?;
        let handle = self.handle(session_id)?;
        require_capture(&handle, CaptureOperation::Resize)?;
        (handle.resize)(cols, rows)?;
        handle
            .parser
            .lock()
            .map_err(|_| SessionManagerError::Lock {
                resource: "screen",
                session_id,
            })?
            .screen_mut()
            .set_size(rows, cols);
        Ok(())
    }

    /// Write terminal input and append its metadata to the audit log.
    pub fn write_input(
        &self,
        session_id: SessionId,
        bytes: &[u8],
        client_addr: &str,
    ) -> Result<(), SessionManagerError> {
        let handle = self.handle(session_id)?;
        require_capture(&handle, CaptureOperation::Input)?;
        self.inner.audit.append(&AuditRecord {
            timestamp: (self.inner.config.clock)(),
            client_addr: client_addr.to_string(),
            session_id,
            byte_count: bytes.len(),
        })?;
        let mut writer = handle
            .writer
            .lock()
            .map_err(|_| SessionManagerError::Lock {
                resource: "PTY writer",
                session_id,
            })?;
        writer
            .write_all(bytes)
            .and_then(|()| writer.flush())
            .map_err(|source| SessionManagerError::File {
                operation: "write PTY input for",
                path: PathBuf::from(format!("session-{}", session_id.0)),
                source,
            })
    }

    /// Kill a session and mark it interrupted without removing its history.
    pub fn kill(&self, session_id: SessionId) -> Result<(), SessionManagerError> {
        let handle = self.handle(session_id)?;
        require_capture(&handle, CaptureOperation::Kill)?;
        (handle.kill)()?;
        let mut session = lock_session(&handle)?;
        if !session.state.is_finished() {
            session.transition(SessionState::Interrupted, (self.inner.config.clock)())?;
            if let Some(store) = &self.inner.store {
                store.update_session(&session)?;
            }
        }
        Ok(())
    }

    fn handle(&self, session_id: SessionId) -> Result<Arc<SessionHandle>, SessionManagerError> {
        self.inner
            .sessions
            .read()
            .map_err(|_| SessionManagerError::Lock {
                resource: "session map",
                session_id,
            })?
            .get(&session_id)
            .cloned()
            .ok_or(SessionManagerError::UnknownSession(session_id))
    }
}

fn validate_size(cols: u16, rows: u16) -> Result<(), SessionManagerError> {
    if cols == 0 || rows == 0 {
        Err(SessionManagerError::InvalidTerminalSize { cols, rows })
    } else {
        Ok(())
    }
}

fn lock_session(
    handle: &SessionHandle,
) -> Result<std::sync::MutexGuard<'_, Session>, SessionManagerError> {
    handle
        .session
        .lock()
        .map_err(|_| SessionManagerError::Lock {
            resource: "session",
            session_id: handle.id,
        })
}

fn system_timestamp() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    Timestamp(i64::try_from(millis).unwrap_or(i64::MAX))
}
