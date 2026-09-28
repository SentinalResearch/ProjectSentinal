//! SHELL_HOOK / COMMANDS Sessions reported by ordinary shells over the local socket.
//!
//! These Sessions have no daemon PTY: no output, input, screen, resize, or
//! kill. Commands arrive as start/finish reports carrying a per-shell
//! sequence number, and the shell PID (plus its OS start time) is the
//! Session's process-tree root. Liveness and recovery are in
//! `external_lifecycle`. Lock order: `shell_reports`, then the
//! tracker and Session locks taken inside `apply_signal`.

use super::{
    lock_session, tasks::apply_signal, SessionHandle, SessionManager, SessionManagerError,
};
use crate::{
    osc::ShellSignal, process_probe::ProcessProbe, pty::PtyError, scrollback::Scrollback,
    tracker::CommandTracker,
};
use protocol::{
    CaptureLevel, Command, LocalErrorCode, LocalRequest, LocalResponse, Session, SessionId,
    SessionOrigin, SessionState, Timestamp,
};
use std::{
    collections::BTreeMap,
    io::Write,
    sync::{atomic::Ordering, Arc, Mutex},
};
use tokio::sync::broadcast;

/// Bounded number of early finishes and recent Commands kept per shell.
const MAX_TRACKED_SEQUENCES: usize = 16;

/// Per-shell Command sequence state.
#[derive(Debug, Default)]
pub(super) struct ShellReports {
    last_started: u64,
    running: Option<u64>,
    /// Finishes that arrived before their start (reporters run concurrently).
    pending_finish: BTreeMap<u64, (i32, Timestamp)>,
    /// Recent Commands by sequence, for idempotent repeated starts.
    recent: BTreeMap<u64, Command>,
}

impl ShellReports {
    /// Drop any running or queued Command state (the shell is gone).
    pub(super) fn forget_running(&mut self) {
        self.running = None;
        self.pending_finish.clear();
    }

    fn remember(&mut self, sequence: u64, command: Command) {
        self.recent.insert(sequence, command);
        while self.recent.len() > MAX_TRACKED_SEQUENCES {
            self.recent.pop_first();
        }
    }
}

/// PTY stand-in whose every operation fails; capture checks run first anyway.
struct NoPty;

impl Write for NoPty {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("COMMANDS Sessions have no PTY"))
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl SessionManager {
    /// Use `probe` to verify external process identity. Call before cloning.
    pub fn with_process_probe(mut self, probe: ProcessProbe) -> Self {
        match Arc::get_mut(&mut self.inner) {
            Some(inner) => inner.probe = probe,
            None => tracing::error!("process probe must be set before the manager is shared"),
        }
        self
    }

    /// Register an ordinary shell, or return its existing live Session.
    pub fn register_shell(
        &self,
        pid: u32,
        shell: &str,
        cwd: &str,
        title: Option<String>,
    ) -> Result<Session, SessionManagerError> {
        let started_at = self.verify_owned_process(pid)?;
        let _registration =
            self.inner
                .registration
                .lock()
                .map_err(|_| SessionManagerError::Lock {
                    resource: "shell registration",
                    session_id: SessionId(0),
                })?;
        for handle in self.handles()? {
            let session = lock_session(&handle)?;
            if session.origin == SessionOrigin::ShellHook
                && session.pid == Some(pid)
                && session.process_started_at == Some(started_at)
                && !session.state.is_finished()
            {
                return Ok(session.clone());
            }
        }

        let id = SessionId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let now = (self.inner.config.clock)();
        let mut session = Session::new(id, self.inner.config.machine_id.clone(), shell, cwd, now);
        session.origin = SessionOrigin::ShellHook;
        session.capture = CaptureLevel::Commands;
        session.title = title;
        session.pid = Some(pid);
        session.process_started_at = Some(started_at);
        session.transition(SessionState::Idle, now)?;
        session.validate()?;
        if let Some(store) = &self.inner.store {
            store.insert_session(&session)?;
        }
        self.insert_external(session.clone())?;
        Ok(session)
    }

    /// Record a Command start reported by a registered shell.
    pub fn shell_command_started(
        &self,
        session_id: SessionId,
        pid: u32,
        sequence: u64,
        text: &str,
        cwd: &str,
        started_at: Timestamp,
    ) -> Result<Command, SessionManagerError> {
        let handle = self.shell_handle(session_id, pid)?;
        let mut reports = lock_reports(&handle)?;
        if sequence <= reports.last_started {
            return reports.recent.get(&sequence).cloned().ok_or(
                SessionManagerError::SequenceConflict {
                    session_id,
                    sequence,
                },
            );
        }
        if reports.running.take().is_some() {
            // The previous finish was lost; a shell runs Commands one at a time.
            if let Ok(mut tracker) = handle.tracker.lock() {
                let _ = tracker.abandon();
            }
        }
        reports.last_started = sequence;
        let store = self.inner.store.as_deref();
        let _ = apply_signal(
            session_id,
            &handle,
            store,
            ShellSignal::Cwd(cwd.into()),
            started_at,
        );
        let _ = apply_signal(
            session_id,
            &handle,
            store,
            ShellSignal::CommandText(text.into()),
            started_at,
        );
        let mut command = apply_signal(
            session_id,
            &handle,
            store,
            ShellSignal::CommandStart,
            started_at,
        )
        .and_then(|update| update.command)
        .ok_or(SessionManagerError::SequenceConflict {
            session_id,
            sequence,
        })?;
        reports.running = Some(sequence);
        if let Some((exit_code, finished_at)) = reports.pending_finish.remove(&sequence) {
            if let Some(finished) = self.finish_running(&handle, exit_code, finished_at, &command) {
                reports.running = None;
                command = finished;
            }
        }
        reports
            .pending_finish
            .retain(|pending, _| *pending > sequence);
        reports.remember(sequence, command.clone());
        Ok(command)
    }

    /// Record a Command finish; `None` means it was queued until its start arrives.
    pub fn shell_command_finished(
        &self,
        session_id: SessionId,
        pid: u32,
        sequence: u64,
        exit_code: i32,
        finished_at: Timestamp,
    ) -> Result<Option<Command>, SessionManagerError> {
        let handle = self.shell_handle(session_id, pid)?;
        let mut reports = lock_reports(&handle)?;
        if reports.running == Some(sequence) {
            let started = reports.recent.get(&sequence).cloned().ok_or(
                SessionManagerError::SequenceConflict {
                    session_id,
                    sequence,
                },
            )?;
            let finished = self
                .finish_running(&handle, exit_code, finished_at, &started)
                .ok_or(SessionManagerError::SequenceConflict {
                    session_id,
                    sequence,
                })?;
            reports.running = None;
            reports.remember(sequence, finished.clone());
            return Ok(Some(finished));
        }
        if sequence > reports.last_started {
            reports
                .pending_finish
                .insert(sequence, (exit_code, finished_at));
            while reports.pending_finish.len() > MAX_TRACKED_SEQUENCES {
                reports.pending_finish.pop_first();
            }
            return Ok(None);
        }
        Err(SessionManagerError::SequenceConflict {
            session_id,
            sequence,
        })
    }

    /// Dispatch one verified local-socket request.
    pub fn handle_local(&self, request: LocalRequest) -> LocalResponse {
        let result = match request {
            LocalRequest::RegisterShell {
                pid,
                shell,
                cwd,
                title,
            } => self
                .register_shell(pid, &shell, &cwd, title)
                .map(|session| LocalResponse::Session { session }),
            LocalRequest::ShellCommandStarted {
                session_id,
                pid,
                sequence,
                text,
                cwd,
                started_at,
            } => self
                .shell_command_started(session_id, pid, sequence, &text, &cwd, started_at)
                .map(|command| LocalResponse::Command { command }),
            LocalRequest::ShellCommandFinished {
                session_id,
                pid,
                sequence,
                exit_code,
                finished_at,
            } => self
                .shell_command_finished(session_id, pid, sequence, exit_code, finished_at)
                .and_then(|finished| match finished {
                    Some(command) => Ok(LocalResponse::Command { command }),
                    None => self
                        .session(session_id)
                        .map(|session| LocalResponse::Session { session }),
                }),
            LocalRequest::AdoptProcess { .. } => {
                return LocalResponse::Error {
                    code: LocalErrorCode::Unavailable,
                    message: "process adoption arrives in Milestone 1b.C".into(),
                };
            }
        };
        result.unwrap_or_else(|error| LocalResponse::Error {
            code: error_code(&error),
            message: error.to_string(),
        })
    }

    fn verify_owned_process(&self, pid: u32) -> Result<Timestamp, SessionManagerError> {
        let probe = &self.inner.probe;
        let owner = probe
            .owner_uid()
            .ok_or(SessionManagerError::ProcessProbeUnavailable)?;
        let identity = probe
            .identify(pid)
            .ok_or(SessionManagerError::ProcessNotFound(pid))?;
        if identity.uid != owner {
            return Err(SessionManagerError::ProcessNotOwned {
                pid,
                uid: identity.uid,
            });
        }
        Ok(identity.started_at)
    }

    fn shell_handle(
        &self,
        session_id: SessionId,
        pid: u32,
    ) -> Result<Arc<SessionHandle>, SessionManagerError> {
        let handle = self.handle(session_id)?;
        let session = lock_session(&handle)?;
        if session.origin != SessionOrigin::ShellHook || session.state.is_finished() {
            return Err(SessionManagerError::NotShellHook(session_id));
        }
        if session.pid != Some(pid) {
            return Err(SessionManagerError::ShellPidMismatch { session_id, pid });
        }
        drop(session);
        Ok(handle)
    }

    /// Finish the tracker's running Command, never before it started.
    fn finish_running(
        &self,
        handle: &SessionHandle,
        exit_code: i32,
        finished_at: Timestamp,
        started: &Command,
    ) -> Option<Command> {
        let at = finished_at.max(started.started_at);
        apply_signal(
            handle.id,
            handle,
            self.inner.store.as_deref(),
            ShellSignal::CommandEnd(exit_code),
            at,
        )
        .and_then(|update| update.command)
    }

    pub(super) fn insert_external(&self, session: Session) -> Result<(), SessionManagerError> {
        let id = session.id;
        let cwd = session.cwd.clone();
        let (output, _) = broadcast::channel(1);
        let (structured, _) = broadcast::channel(64);
        let unsupported = move |operation| PtyError::Operation {
            operation,
            session_id: id,
            message: "COMMANDS Sessions have no daemon-owned PTY".into(),
        };
        let handle = Arc::new(SessionHandle {
            id,
            root_pid: session.pid,
            session: Mutex::new(session),
            writer: Arc::new(Mutex::new(Box::new(NoPty))),
            resize: Arc::new(move |_, _| Err(unsupported("resize"))),
            kill: Arc::new(move || Err(unsupported("kill"))),
            parser: Arc::new(Mutex::new(vt100::Parser::new(1, 1, 0))),
            scrollback: Arc::new(Mutex::new(Scrollback::new(0))),
            output,
            structured,
            tracker: Mutex::new(CommandTracker::new(
                self.inner.config.machine_id.clone(),
                cwd,
                self.inner.tracker_ids.clone(),
            )),
            initial_command: Mutex::new(None),
            shell_reports: Mutex::default(),
        });
        self.inner
            .sessions
            .write()
            .map_err(|_| SessionManagerError::Lock {
                resource: "session map",
                session_id: id,
            })?
            .insert(id, handle);
        Ok(())
    }

    pub(super) fn handles(&self) -> Result<Vec<Arc<SessionHandle>>, SessionManagerError> {
        Ok(self
            .inner
            .sessions
            .read()
            .map_err(|_| SessionManagerError::Lock {
                resource: "session map",
                session_id: SessionId(0),
            })?
            .values()
            .cloned()
            .collect())
    }
}

pub(super) fn lock_reports(
    handle: &SessionHandle,
) -> Result<std::sync::MutexGuard<'_, ShellReports>, SessionManagerError> {
    handle
        .shell_reports
        .lock()
        .map_err(|_| SessionManagerError::Lock {
            resource: "shell reports",
            session_id: handle.id,
        })
}

fn error_code(error: &SessionManagerError) -> LocalErrorCode {
    match error {
        SessionManagerError::UnknownSession(_) | SessionManagerError::ProcessNotFound(_) => {
            LocalErrorCode::NotFound
        }
        SessionManagerError::ProcessNotOwned { .. }
        | SessionManagerError::NotShellHook(_)
        | SessionManagerError::ShellPidMismatch { .. } => LocalErrorCode::Forbidden,
        SessionManagerError::SequenceConflict { .. } => LocalErrorCode::Conflict,
        SessionManagerError::ProcessProbeUnavailable => LocalErrorCode::Unavailable,
        SessionManagerError::Protocol(_) => LocalErrorCode::InvalidRequest,
        _ => LocalErrorCode::Internal,
    }
}
