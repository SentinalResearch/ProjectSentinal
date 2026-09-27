//! Ownership and lifecycle management for persistent terminal sessions.

mod tasks;

use crate::{
    audit::{AuditLog, AuditRecord},
    pty::{PtyBackend, PtyError, PtySpawnRequest, SpawnedPty},
    scrollback::Scrollback,
};
use protocol::{MachineId, ProtocolError, Session, SessionId, SessionState, Timestamp};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};
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
}

/// A screen snapshot followed by a receiver for live terminal bytes.
pub struct Attachment {
    /// ANSI bytes sufficient to redraw the current screen.
    pub screen: Vec<u8>,
    /// Live output produced after attachment.
    pub output: broadcast::Receiver<Vec<u8>>,
}

/// Errors produced while managing sessions.
#[derive(Debug, thiserror::Error)]
pub enum SessionManagerError {
    /// A session ID is not owned by this daemon.
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    /// A terminal dimension was zero.
    #[error("terminal size must be at least 1x1, got {cols}x{rows}")]
    InvalidTerminalSize {
        /// Requested columns.
        cols: u16,
        /// Requested rows.
        rows: u16,
    },
    /// A PTY operation failed.
    #[error(transparent)]
    Pty(#[from] PtyError),
    /// A session state transition failed.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// A session data file operation failed.
    #[error("failed to {operation} session data '{}': {source}", path.display())]
    File {
        /// Operation that failed.
        operation: &'static str,
        /// Data path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Shared session state was poisoned by a panic.
    #[error("failed to lock {resource} for session {session_id}: lock poisoned")]
    Lock {
        /// Resource being accessed.
        resource: &'static str,
        /// Session being accessed.
        session_id: SessionId,
    },
    /// A remote input audit record could not be written.
    #[error(transparent)]
    Audit(#[from] crate::audit::AuditError),
}

struct SessionHandle {
    id: SessionId,
    session: Mutex<Session>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    resize: Arc<dyn Fn(u16, u16) -> Result<(), PtyError> + Send + Sync>,
    kill: Arc<dyn Fn() -> Result<(), PtyError> + Send + Sync>,
    parser: Arc<Mutex<vt100::Parser>>,
    scrollback: Arc<Mutex<Scrollback>>,
    output: broadcast::Sender<Vec<u8>>,
}

struct SessionManagerInner {
    backend: Arc<dyn PtyBackend>,
    config: SessionManagerConfig,
    audit: AuditLog,
    sessions: RwLock<HashMap<SessionId, Arc<SessionHandle>>>,
    next_id: AtomicU64,
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
        SessionManager {
            inner: Arc::new(SessionManagerInner {
                backend,
                config,
                audit,
                sessions: RwLock::new(HashMap::new()),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    /// Spawn a shell and return its session in the idle state.
    pub async fn create(&self, request: CreateSession) -> Result<Session, SessionManagerError> {
        validate_size(request.cols, request.rows)?;
        let id = SessionId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let shell = request
            .shell
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".into());
        let cwd = request
            .cwd
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let log_dir = self.inner.config.data_dir.join("sessions");
        fs::create_dir_all(&log_dir).map_err(|source| SessionManagerError::File {
            operation: "create directory for",
            path: log_dir.clone(),
            source,
        })?;
        let log_path = log_dir.join(format!("{}.log.zst", id.0));
        let log_file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .map_err(|source| SessionManagerError::File {
                operation: "open log",
                path: log_path.clone(),
                source,
            })?;

        let spawned = self.inner.backend.spawn(&PtySpawnRequest {
            session_id: id,
            shell: shell.clone(),
            cwd: cwd.clone(),
            cols: request.cols,
            rows: request.rows,
        })?;
        let mut session = Session::new(
            id,
            self.inner.config.machine_id.clone(),
            shell,
            cwd.to_string_lossy(),
            (self.inner.config.clock)(),
        );
        session.transition(SessionState::Idle, (self.inner.config.clock)())?;

        let (output, _) = broadcast::channel(256);
        let parser = Arc::new(Mutex::new(vt100::Parser::new(
            request.rows,
            request.cols,
            self.inner.config.scrollback_lines,
        )));
        let scrollback = Arc::new(Mutex::new(Scrollback::new(
            self.inner.config.scrollback_lines,
        )));
        let SpawnedPty {
            reader,
            writer,
            resize,
            kill,
            try_wait,
        } = spawned;
        let handle = Arc::new(SessionHandle {
            id,
            session: Mutex::new(session.clone()),
            writer,
            resize,
            kill,
            parser: parser.clone(),
            scrollback: scrollback.clone(),
            output: output.clone(),
        });
        self.inner
            .sessions
            .write()
            .map_err(|_| SessionManagerError::Lock {
                resource: "session map",
                session_id: id,
            })?
            .insert(id, handle.clone());

        tasks::spawn_reader(id, reader, parser, scrollback, output, log_path, log_file);
        tasks::spawn_exit_monitor(id, handle, try_wait, self.inner.config.clock.clone());
        Ok(session)
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
        (handle.kill)()?;
        let mut session = lock_session(&handle)?;
        if !session.state.is_finished() {
            session.transition(SessionState::Interrupted, (self.inner.config.clock)())?;
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
