//! Ownership and lifecycle management for persistent terminal sessions.

mod adopt;
mod adopted_log;
mod capture;
mod continuation;
mod error;
mod external;
mod external_lifecycle;
mod lifecycle;
mod local_dispatch;
mod log_budget;
mod operations;
mod replay;
mod structured;
mod tasks;

use crate::{
    audit::AuditLog,
    process_probe::ProcessProbe,
    pty::{PtyBackend, PtyError},
    scrollback::Scrollback,
    tracker::{CommandTracker, TrackerIds},
};
pub use error::SessionManagerError;
pub use operations::{ScreenSource, RESET_TERMINAL};
use protocol::{DaemonMessage, Event, MachineId, Session, SessionId, SessionOrigin, Timestamp};
pub use replay::{
    OutputReplay, DEFAULT_OUTPUT_PAGE_BYTES, MAX_OUTPUT_PAGE_BYTES, MAX_OUTPUT_SCAN_BYTES,
};
use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Arc, Mutex, RwLock,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use store::Store;
use tokio::sync::broadcast;

/// How long an AUTO_WRAP Session may wait for its terminal to attach.
pub const AUTO_WRAP_ATTACH_GRACE: std::time::Duration = std::time::Duration::from_millis(300);

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
    /// The starting terminal's environment (AUTO_WRAP and WATCH only); the
    /// shell starts from it instead of the daemon's environment.
    pub env: Option<Vec<(String, String)>>,
}

/// A screen snapshot followed by a receiver for live terminal bytes.
pub struct Attachment {
    /// ANSI bytes sufficient to redraw the current screen.
    pub screen: Vec<u8>,
    /// Live output produced after attachment.
    pub output: broadcast::Receiver<Vec<u8>>,
    /// Structured command, event, Session, and metric updates.
    pub structured: broadcast::Receiver<DaemonMessage>,
    pub(crate) confirmation: Arc<AtomicBool>,
    /// Redraws this viewer if it falls behind `output`.
    pub screen_source: ScreenSource,
}

impl Attachment {
    /// Confirm that the screen snapshot reached the attaching transport.
    pub fn confirm(&self) {
        self.confirmation
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

struct SessionHandle {
    id: SessionId,
    root_pid: Option<u32>,
    session: Mutex<Session>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    resize: Arc<dyn Fn(u16, u16) -> Result<(), PtyError> + Send + Sync>,
    kill: Arc<dyn Fn() -> Result<(), PtyError> + Send + Sync>,
    try_wait: Arc<dyn Fn() -> Result<Option<i32>, PtyError> + Send + Sync>,
    pty_closed: AtomicBool,
    parser: Arc<Mutex<vt100::Parser>>,
    scrollback: Arc<Mutex<Scrollback>>,
    output: broadcast::Sender<Vec<u8>>,
    structured: broadcast::Sender<DaemonMessage>,
    /// Global observation stream used by Sessions that cannot attach.
    observations: broadcast::Sender<DaemonMessage>,
    /// Machine-wide Event stream consumed by local notification delivery.
    events: broadcast::Sender<Event>,
    tracker: Mutex<CommandTracker>,
    /// Deterministic Event and Health state for this Session.
    watcher: Mutex<watcher::EventEngine>,
    /// ANSI-free bounded line framing shared by PTY and log observations.
    line_framer: Mutex<watcher::LineFramer>,
    /// Shared allocator for watcher-produced Event IDs.
    watcher_ids: TrackerIds,
    /// Rendered Watch initial Command, taken and typed at the first prompt.
    initial_command: Mutex<Option<Vec<u8>>>,
    /// Sequence bookkeeping for Commands reported by an external shell hook.
    shell_reports: Mutex<external::ShellReports>,
    /// Set by the first successful attach.
    attached: Arc<AtomicBool>,
    /// Active read-only tail of an adopted process's log.
    log_tail: Mutex<Option<adopted_log::ActiveLogTail>>,
    /// Descendants of an adopted process, which keep it alive after its root exits.
    adopted_tree: Mutex<adopted_log::AdoptedTree>,
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
    /// Metrics and state of reduced-capture Sessions, for every connected
    /// client (they cannot attach, which is how FULL Sessions deliver them).
    observations: broadcast::Sender<DaemonMessage>,
    /// Machine-wide Metrics stream consumed by the projection recorder.
    projection_observations: broadcast::Sender<DaemonMessage>,
    /// Machine-wide structured Event stream.
    events: broadcast::Sender<Event>,
    /// Serializes external shell registration so concurrent reports create one Session.
    registration: Mutex<()>,
    /// Serializes continuation so a source can never branch in one daemon.
    continuation: tokio::sync::Mutex<()>,
    /// Validated user-defined watcher adapters cloned into each Session engine.
    watcher_adapters: Vec<watcher::DeclarativeAdapter>,
    /// Whether adopted log tails poll on a background timer (tests drive them).
    log_tail_polling: bool,
}

/// Owner of all persistent sessions on one daemon.
#[derive(Clone)]
pub struct SessionManager {
    inner: Arc<SessionManagerInner>,
}

impl SessionManager {
    /// Subscribe to Events emitted by every Session on this Machine.
    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

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
                observations: broadcast::channel(256).0,
                projection_observations: broadcast::channel(256).0,
                events: broadcast::channel(256).0,
                registration: Mutex::new(()),
                continuation: tokio::sync::Mutex::new(()),
                watcher_adapters: Vec::new(),
                log_tail_polling: true,
            }),
        }
    }

    /// Disable background log-tail polling so tests can call `poll_log_tail`.
    pub fn with_log_tail_polling(mut self, enabled: bool) -> Self {
        match Arc::get_mut(&mut self.inner) {
            Some(inner) => inner.log_tail_polling = enabled,
            None => tracing::error!("log-tail polling must be set before the manager is shared"),
        }
        self
    }

    /// Install validated declarative adapters before this manager is shared.
    pub fn with_watcher_adapters(mut self, adapters: Vec<watcher::DeclarativeAdapter>) -> Self {
        match Arc::get_mut(&mut self.inner) {
            Some(inner) => inner.watcher_adapters = adapters,
            None => tracing::error!("watcher adapters must be set before the manager is shared"),
        }
        self
    }

    /// Return all sessions ordered by ID.
    pub fn list(&self) -> Result<Vec<Session>, SessionManagerError> {
        let mut by_id = self
            .inner
            .store
            .as_ref()
            .map(|store| store.sessions())
            .transpose()?
            .unwrap_or_default()
            .into_iter()
            .map(|session| (session.id, session))
            .collect::<HashMap<_, _>>();
        let sessions = self
            .inner
            .sessions
            .read()
            .map_err(|_| SessionManagerError::Lock {
                resource: "session map",
                session_id: SessionId(0),
            })?;
        for handle in sessions.values() {
            let session = lock_session(handle)?.clone();
            by_id.insert(session.id, session);
        }
        let mut result = by_id.into_values().collect::<Vec<_>>();
        result.sort_by_key(|session| session.id);
        Ok(result)
    }

    /// Return one session's current structured state.
    pub fn session(&self, session_id: SessionId) -> Result<Session, SessionManagerError> {
        let handle = self.handle(session_id)?;
        let session = lock_session(&handle)?.clone();
        Ok(session)
    }

    fn known_session(&self, session_id: SessionId) -> Result<Session, SessionManagerError> {
        match self.session(session_id) {
            Ok(session) => return Ok(session),
            Err(SessionManagerError::UnknownSession(_)) => {}
            Err(error) => return Err(error),
        }
        let persisted = self
            .inner
            .store
            .as_ref()
            .map(|store| store.session(session_id))
            .transpose()?
            .flatten();
        persisted.ok_or(SessionManagerError::UnknownSession(session_id))
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
