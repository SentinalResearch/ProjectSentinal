//! PTY creation behind real and deterministic fake backends.

use protocol::SessionId;
use std::{
    collections::HashMap,
    io::{Cursor, Read, Write},
    path::PathBuf,
    sync::{mpsc, Arc, Mutex},
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
}

/// PTY backend backed by the host operating system.
#[derive(Debug, Default)]
pub struct RealPty;

impl PtyBackend for RealPty {
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
    output: Option<mpsc::Sender<Vec<u8>>>,
    input: Arc<Mutex<Vec<u8>>>,
    size: Arc<Mutex<(u16, u16)>>,
    status: Arc<Mutex<Option<i32>>>,
}

/// Deterministic PTY backend controlled directly by tests.
#[derive(Debug, Default)]
pub struct FakePty {
    sessions: Arc<Mutex<HashMap<SessionId, FakeControl>>>,
}

impl FakePty {
    /// Create an empty fake backend.
    pub fn new() -> Self {
        Self::default()
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
        control.output.take();
        Ok(())
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
    fn spawn(&self, request: &PtySpawnRequest) -> Result<SpawnedPty, PtyError> {
        let (sender, receiver) = mpsc::channel();
        let input = Arc::new(Mutex::new(Vec::new()));
        let size = Arc::new(Mutex::new((request.cols, request.rows)));
        let status = Arc::new(Mutex::new(None));
        let session_id = request.session_id;

        self.sessions
            .lock()
            .map_err(|_| {
                operation_error(session_id, "lock fake backend to spawn", "lock poisoned")
            })?
            .insert(
                session_id,
                FakeControl {
                    output: Some(sender),
                    input: input.clone(),
                    size: size.clone(),
                    status: status.clone(),
                },
            );

        let resize_size = size.clone();
        let resize = Arc::new(move |cols, rows| {
            *resize_size.lock().map_err(|_| {
                operation_error(session_id, "lock fake size for", "lock poisoned")
            })? = (cols, rows);
            Ok(())
        });

        let kill_sessions = self.sessions.clone();
        let kill_status = status.clone();
        let kill = Arc::new(move || {
            *kill_status.lock().map_err(|_| {
                operation_error(
                    session_id,
                    "lock fake child status to kill",
                    "lock poisoned",
                )
            })? = Some(130);
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
            writer: Arc::new(Mutex::new(Box::new(FakeWriter { input }))),
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
}

impl Write for FakeWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.input
            .lock()
            .map_err(|_| std::io::Error::other("fake PTY input lock poisoned"))?
            .extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
