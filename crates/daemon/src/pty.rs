//! PTY creation behind real and deterministic fake backends.

use protocol::SessionId;
use std::{
    collections::HashMap,
    io::{Cursor, Read, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
};

/// Parameters needed to start a shell in a PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtySpawnRequest {
    /// Session being started.
    pub session_id: SessionId,
    /// Shell executable.
    pub shell: String,
    /// Additional arguments used to load shell hooks.
    pub args: Vec<String>,
    /// The environment to start from instead of the daemon's own (a
    /// terminal's, for AUTO_WRAP and WATCH); `env` is applied on top.
    pub base_env: Option<Vec<(String, String)>>,
    /// Additional environment entries used to load shell hooks.
    pub env: Vec<(String, String)>,
    /// Initial working directory.
    pub cwd: PathBuf,
    /// Initial terminal width.
    pub cols: u16,
    /// Initial terminal height.
    pub rows: u16,
}

type ResizeFn = dyn Fn(u16, u16) -> Result<(), PtyError> + Send + Sync;
type ControlFn = dyn Fn() -> Result<(), PtyError> + Send + Sync;
type TryWaitFn = dyn Fn() -> Result<Option<i32>, PtyError> + Send + Sync;

/// The I/O and control capabilities of one spawned PTY.
pub struct SpawnedPty {
    pub(crate) root_pid: Option<u32>,
    pub(crate) reader: Box<dyn Read + Send>,
    pub(crate) writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub(crate) resize: Arc<ResizeFn>,
    pub(crate) kill: Arc<ControlFn>,
    pub(crate) try_wait: Arc<TryWaitFn>,
}

/// Errors produced by PTY backends.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PtyError {
    /// A backend operation failed.
    #[error("failed to {operation} PTY for session {session_id}: {message}")]
    Operation {
        /// Operation that failed.
        operation: &'static str,
        /// Session being controlled.
        session_id: SessionId,
        /// Backend error details.
        message: String,
    },
    /// The fake backend does not know the requested session.
    #[error("fake PTY has no session {0}")]
    UnknownSession(SessionId),
}

/// A source of PTYs for persistent sessions.
pub trait PtyBackend: Send + Sync {
    /// Spawn one shell and return its PTY capabilities.
    fn spawn(&self, request: &PtySpawnRequest) -> Result<SpawnedPty, PtyError>;

    /// The user's login environment for `shell`, for Sessions no terminal
    /// started; `None` keeps the daemon's own environment.
    fn login_env(&self, _shell: &str) -> Option<Vec<(String, String)>> {
        None
    }
}

/// PTY backend backed by the host operating system.
#[derive(Debug, Default)]
pub struct RealPty;

impl PtyBackend for RealPty {
    fn login_env(&self, shell: &str) -> Option<Vec<(String, String)>> {
        crate::login_env::resolve(
            std::path::Path::new(shell),
            crate::login_env::RESOLVE_TIMEOUT,
        )
    }

    fn spawn(&self, request: &PtySpawnRequest) -> Result<SpawnedPty, PtyError> {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};

        let pair = native_pty_system()
            .openpty(PtySize {
                rows: request.rows,
                cols: request.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| operation_error(request.session_id, "open", error))?;
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| operation_error(request.session_id, "clone reader for", error))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| operation_error(request.session_id, "take writer for", error))?;

        let mut command = CommandBuilder::new(&request.shell);
        command.args(&request.args);
        if let Some(base) = &request.base_env {
            command.env_clear();
            for (key, value) in base {
                command.env(key, value);
            }
        }
        for (key, value) in &request.env {
            command.env(key, value);
        }
        command.cwd(&request.cwd);
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| operation_error(request.session_id, "spawn shell in", error))?;
        let root_pid = child.process_id();
        let killer = child.clone_killer();
        let child = Arc::new(Mutex::new(child));
        let killer = Arc::new(Mutex::new(killer));
        let master = Arc::new(Mutex::new(pair.master));

        let resize_master = master.clone();
        let session_id = request.session_id;
        let resize = Arc::new(move |cols, rows| {
            let master = resize_master.lock().map_err(|_| PtyError::Operation {
                operation: "lock master to resize",
                session_id,
                message: "lock poisoned".into(),
            })?;
            master
                .resize(PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|error| operation_error(session_id, "resize", error))
        });

        let kill_child = killer.clone();
        let kill = Arc::new(move || {
            kill_child
                .lock()
                .map_err(|_| PtyError::Operation {
                    operation: "lock child to kill",
                    session_id,
                    message: "lock poisoned".into(),
                })?
                .kill()
                .map_err(|error| operation_error(session_id, "kill", error))
        });

        let wait_child = child.clone();
        let try_wait = Arc::new(move || {
            let status = wait_child
                .lock()
                .map_err(|_| PtyError::Operation {
                    operation: "lock child to poll",
                    session_id,
                    message: "lock poisoned".into(),
                })?
                .try_wait()
                .map_err(|error| operation_error(session_id, "poll", error))?;
            status
                .map(|status| {
                    i32::try_from(status.exit_code()).map_err(|error| PtyError::Operation {
                        operation: "convert exit code for",
                        session_id,
                        message: error.to_string(),
                    })
                })
                .transpose()
        });

        Ok(SpawnedPty {
            root_pid,
            reader,
            writer: Arc::new(Mutex::new(writer)),
            resize,
            kill,
            try_wait,
        })
    }
}

fn operation_error(
    session_id: SessionId,
    operation: &'static str,
    error: impl std::fmt::Display,
) -> PtyError {
    PtyError::Operation {
        operation,
        session_id,
        message: error.to_string(),
    }
}

#[derive(Debug)]
struct FakeControl {
    request: PtySpawnRequest,
    output: Option<mpsc::Sender<Vec<u8>>>,
    input: Arc<Mutex<Vec<u8>>>,
    size: Arc<Mutex<(u16, u16)>>,
    status: Arc<Mutex<Option<i32>>>,
    closed: Arc<AtomicBool>,
    writes: Arc<AtomicUsize>,
    resizes: Arc<AtomicUsize>,
    kills: Arc<AtomicUsize>,
}

/// Counts of fake PTY operations used to prove lifecycle gates have no side effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakePtyOperationCounts {
    /// Number of attempted writes.
    pub writes: usize,
    /// Number of attempted resizes.
    pub resizes: usize,
    /// Number of attempted kills.
    pub kills: usize,
}

type FakeSessions = Arc<Mutex<HashMap<SessionId, FakeControl>>>;

/// Deterministic PTY backend controlled directly by tests.
#[derive(Debug, Default)]
pub struct FakePty {
    sessions: FakeSessions,
    login_env: Mutex<Option<Vec<(String, String)>>>,
    echo: bool,
}

impl FakePty {
    /// Create an empty fake backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// A fake whose PTYs echo input back as output, like a terminal in echo
    /// mode (`TA_FAKE_PTY=echo`), so browser tests can time keystroke echoes.
    pub fn echoing() -> Self {
        FakePty {
            echo: true,
            ..Self::default()
        }
    }

    /// Pretend the user's login shell provides `env`.
    pub fn set_login_env(&self, env: Vec<(String, String)>) {
        if let Ok(mut login_env) = self.login_env.lock() {
            *login_env = Some(env);
        }
    }

    /// Emit bytes as if a child wrote them to its PTY.
    pub fn output(&self, session_id: SessionId, bytes: &[u8]) -> Result<(), PtyError> {
        let sessions = self.sessions.lock().map_err(|_| {
            operation_error(
                session_id,
                "lock fake backend for output on",
                "lock poisoned",
            )
        })?;
        let control = sessions
            .get(&session_id)
            .ok_or(PtyError::UnknownSession(session_id))?;
        let sender = control.output.as_ref().ok_or_else(|| {
            operation_error(session_id, "write output after exit on", "child exited")
        })?;
        sender
            .send(bytes.to_vec())
            .map_err(|error| operation_error(session_id, "send output on", error))
    }

    /// Finish a fake child with the given exit code and close its output.
    pub fn exit(&self, session_id: SessionId, code: i32) -> Result<(), PtyError> {
        let mut sessions = self.sessions.lock().map_err(|_| {
            operation_error(session_id, "lock fake backend to exit", "lock poisoned")
        })?;
        let control = sessions
            .get_mut(&session_id)
            .ok_or(PtyError::UnknownSession(session_id))?;
        *control.status.lock().map_err(|_| {
            operation_error(session_id, "lock fake child status for", "lock poisoned")
        })? = Some(code);
        control.closed.store(true, Ordering::SeqCst);
        control.output.take();
        Ok(())
    }

    /// Return attempted control operations for one fake PTY.
    pub fn operation_counts(
        &self,
        session_id: SessionId,
    ) -> Result<FakePtyOperationCounts, PtyError> {
        let sessions = self.sessions.lock().map_err(|_| {
            operation_error(
                session_id,
                "lock fake backend for operation counts on",
                "lock poisoned",
            )
        })?;
        let control = sessions
            .get(&session_id)
            .ok_or(PtyError::UnknownSession(session_id))?;
        Ok(FakePtyOperationCounts {
            writes: control.writes.load(Ordering::SeqCst),
            resizes: control.resizes.load(Ordering::SeqCst),
            kills: control.kills.load(Ordering::SeqCst),
        })
    }

    /// Return every byte written to a fake child's input.
    pub fn input(&self, session_id: SessionId) -> Result<Vec<u8>, PtyError> {
        let sessions = self.sessions.lock().map_err(|_| {
            operation_error(
                session_id,
                "lock fake backend for input on",
                "lock poisoned",
            )
        })?;
        let control = sessions
            .get(&session_id)
            .ok_or(PtyError::UnknownSession(session_id))?;
        let bytes = control
            .input
            .lock()
            .map_err(|_| operation_error(session_id, "lock fake input for", "lock poisoned"))?
            .clone();
        Ok(bytes)
    }

    /// Return the spawn request used to start a fake child.
    pub fn spawn_request(&self, session_id: SessionId) -> Result<PtySpawnRequest, PtyError> {
        let sessions = self.sessions.lock().map_err(|_| {
            operation_error(
                session_id,
                "lock fake backend for spawn request on",
                "lock poisoned",
            )
        })?;
        sessions
            .get(&session_id)
            .map(|control| control.request.clone())
            .ok_or(PtyError::UnknownSession(session_id))
    }

    /// Return the last terminal size requested for a fake child.
    pub fn size(&self, session_id: SessionId) -> Result<(u16, u16), PtyError> {
        let sessions = self.sessions.lock().map_err(|_| {
            operation_error(session_id, "lock fake backend for size on", "lock poisoned")
        })?;
        let control = sessions
            .get(&session_id)
            .ok_or(PtyError::UnknownSession(session_id))?;
        let size = *control
            .size
            .lock()
            .map_err(|_| operation_error(session_id, "lock fake size for", "lock poisoned"))?;
        Ok(size)
    }
}

impl PtyBackend for FakePty {
    fn login_env(&self, _shell: &str) -> Option<Vec<(String, String)>> {
        self.login_env.lock().ok()?.clone()
    }

    fn spawn(&self, request: &PtySpawnRequest) -> Result<SpawnedPty, PtyError> {
        let (sender, receiver) = mpsc::channel();
        let input = Arc::new(Mutex::new(Vec::new()));
        let size = Arc::new(Mutex::new((request.cols, request.rows)));
        let status = Arc::new(Mutex::new(None));
        let closed = Arc::new(AtomicBool::new(false));
        let writes = Arc::new(AtomicUsize::new(0));
        let resizes = Arc::new(AtomicUsize::new(0));
        let kills = Arc::new(AtomicUsize::new(0));
        let session_id = request.session_id;

        self.sessions
            .lock()
            .map_err(|_| {
                operation_error(session_id, "lock fake backend to spawn", "lock poisoned")
            })?
            .insert(
                session_id,
                FakeControl {
                    request: request.clone(),
                    output: Some(sender),
                    input: input.clone(),
                    size: size.clone(),
                    status: status.clone(),
                    closed: closed.clone(),
                    writes: writes.clone(),
                    resizes: resizes.clone(),
                    kills: kills.clone(),
                },
            );

        let resize_size = size.clone();
        let resize_count = resizes.clone();
        let resize = Arc::new(move |cols, rows| {
            resize_count.fetch_add(1, Ordering::SeqCst);
            *resize_size.lock().map_err(|_| {
                operation_error(session_id, "lock fake size for", "lock poisoned")
            })? = (cols, rows);
            Ok(())
        });

        let kill_sessions = self.sessions.clone();
        let kill_status = status.clone();
        let kill_closed = closed.clone();
        let kill_count = kills.clone();
        let kill = Arc::new(move || {
            kill_count.fetch_add(1, Ordering::SeqCst);
            *kill_status.lock().map_err(|_| {
                operation_error(
                    session_id,
                    "lock fake child status to kill",
                    "lock poisoned",
                )
            })? = Some(130);
            kill_closed.store(true, Ordering::SeqCst);
            let mut sessions = kill_sessions.lock().map_err(|_| {
                operation_error(session_id, "lock fake backend to kill", "lock poisoned")
            })?;
            if let Some(control) = sessions.get_mut(&session_id) {
                control.output.take();
            }
            Ok(())
        });

        let wait_status = status.clone();
        let try_wait = Arc::new(move || {
            wait_status
                .lock()
                .map_err(|_| {
                    operation_error(
                        session_id,
                        "lock fake child status to poll",
                        "lock poisoned",
                    )
                })
                .map(|status| *status)
        });

        Ok(SpawnedPty {
            root_pid: u32::try_from(session_id.0)
                .ok()
                .and_then(|id| 10_000_u32.checked_add(id)),
            reader: Box::new(FakeReader {
                receiver,
                current: Cursor::new(Vec::new()),
            }),
            writer: Arc::new(Mutex::new(Box::new(FakeWriter {
                input,
                closed,
                writes,
                echo: self.echo.then(|| (self.sessions.clone(), session_id)),
            }))),
            resize,
            kill,
            try_wait,
        })
    }
}

struct FakeReader {
    receiver: mpsc::Receiver<Vec<u8>>,
    current: Cursor<Vec<u8>>,
}

impl Read for FakeReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.current.position() as usize >= self.current.get_ref().len() {
            match self.receiver.recv() {
                Ok(bytes) => self.current = Cursor::new(bytes),
                Err(_) => return Ok(0),
            }
        }
        self.current.read(buffer)
    }
}

struct FakeWriter {
    input: Arc<Mutex<Vec<u8>>>,
    closed: Arc<AtomicBool>,
    writes: Arc<AtomicUsize>,
    /// Echo through the Session's live output channel (none after a kill).
    echo: Option<(FakeSessions, SessionId)>,
}

impl Write for FakeWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        if self.closed.load(Ordering::SeqCst) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "fake PTY is closed",
            ));
        }
        self.input
            .lock()
            .map_err(|_| std::io::Error::other("fake PTY input lock poisoned"))?
            .extend_from_slice(buffer);
        if let Some((sessions, session_id)) = &self.echo {
            if let Some(output) = sessions.lock().ok().and_then(|sessions| {
                sessions
                    .get(session_id)
                    .and_then(|control| control.output.clone())
            }) {
                let _ = output.send(buffer.to_vec());
            }
        }
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
