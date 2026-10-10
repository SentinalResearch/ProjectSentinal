//! Session creation, continuation, and PTY task startup.

use super::{
    lock_session, tasks, validate_size, CreateSession, SessionHandle, SessionManager,
    SessionManagerError, AUTO_WRAP_ATTACH_GRACE,
};
use crate::{
    osc::{new_nonce, OscParser, NONCE_ENV},
    private_fs::{ensure_private_dir, private_file_options},
    pty::{PtySpawnRequest, SpawnedPty},
    scrollback::Scrollback,
    shell::{install_hooks, launch, ShellLaunch},
    tracker::CommandTracker,
};
use protocol::{
    render_command_line, CaptureLevel, ProjectId, Session, SessionId, SessionOrigin, SessionState,
    ShellDialect,
};
use std::{
    fs::{self, File},
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tokio::sync::broadcast;

struct PreparedSession {
    session: Session,
    handle: Arc<SessionHandle>,
    reader: Box<dyn Read + Send>,
    log_path: PathBuf,
    log_file: File,
    osc_nonce: String,
}

#[derive(Clone, Copy)]
enum Persistence {
    Ordinary,
    Continuation(SessionId),
}

impl SessionManager {
    /// Spawn a shell and return its session in the idle state.
    pub async fn create(&self, request: CreateSession) -> Result<Session, SessionManagerError> {
        let prepared = self.prepare(request, None, None, None)?;
        self.publish(prepared, Persistence::Ordinary)
    }

    /// Spawn a Watch Session and type one already safely rendered Recipe command.
    pub async fn create_recipe(
        &self,
        rendered: String,
        cwd: Option<PathBuf>,
        title: String,
    ) -> Result<Session, SessionManagerError> {
        let mut command = rendered.into_bytes();
        command.push(b'\r');
        let prepared = self.prepare(
            CreateSession {
                shell: None,
                cwd,
                cols: 120,
                rows: 40,
                origin: SessionOrigin::Watch,
                title: Some(title),
                command: None,
            },
            None,
            None,
            Some(command),
        )?;
        self.publish(prepared, Persistence::Ordinary)
    }

    pub(super) fn spawn_continuation(
        &self,
        source: Session,
        title: Option<String>,
    ) -> Result<Session, SessionManagerError> {
        let source_id = source.id;
        let prepared = self.prepare(
            CreateSession {
                shell: Some(source.shell),
                cwd: Some(PathBuf::from(source.cwd)),
                cols: 80,
                rows: 24,
                origin: SessionOrigin::Ui,
                title,
                command: None,
            },
            source.project_id,
            Some(source_id),
            None,
        )?;
        self.publish(prepared, Persistence::Continuation(source_id))
    }

    fn prepare(
        &self,
        request: CreateSession,
        project_id: Option<ProjectId>,
        continued_from: Option<SessionId>,
        rendered_command: Option<Vec<u8>>,
    ) -> Result<PreparedSession, SessionManagerError> {
        validate_size(request.cols, request.rows)?;
        if !matches!(
            request.origin,
            SessionOrigin::Ui | SessionOrigin::Watch | SessionOrigin::AutoWrap
        ) {
            return Err(SessionManagerError::OriginUnavailable {
                origin: request.origin,
            });
        }
        let shell = request
            .shell
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".into());
        let initial_command = match rendered_command {
            Some(command) => Some(command),
            None => initial_command(request.origin, &shell, request.command.as_deref())?,
        };
        let cwd = request
            .cwd
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.clone());
        let mut shell_launch = match install_hooks(&self.inner.config.data_dir, &home) {
            Ok(hook_dir) => launch(PathBuf::from(&shell).as_path(), &hook_dir),
            Err(error) if initial_command.is_some() => {
                return Err(SessionManagerError::InitialCommandHooks(error.to_string()));
            }
            Err(error) => {
                tracing::warn!(%error, "shell hooks unavailable; starting Session without command tracking");
                ShellLaunch::default()
            }
        };
        let id = SessionId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        shell_launch
            .env
            .push(("TERM".into(), "xterm-256color".into()));
        shell_launch
            .env
            .push(("COLORTERM".into(), "truecolor".into()));
        shell_launch
            .env
            .push(("SENTINAL_SESSION_ID".into(), id.0.to_string()));
        // Only the hook learns this secret, so output cannot forge shell signals.
        let osc_nonce = new_nonce().map_err(SessionManagerError::OscNonce)?;
        shell_launch.env.push((NONCE_ENV.into(), osc_nonce.clone()));
        if request.origin == SessionOrigin::AutoWrap {
            shell_launch
                .env
                .push(("SENTINAL_AUTO_GUARD".into(), "1".into()));
        }

        let log_dir = self.inner.config.data_dir.join("sessions");
        ensure_private_dir(&log_dir).map_err(|source| SessionManagerError::File {
            operation: "create private directory for",
            path: log_dir.clone(),
            source,
        })?;
        let log_path = log_dir.join(format!("{}.log.zst", id.0));
        let log_file = private_file_options()
            .create_new(true)
            .append(true)
            .open(&log_path)
            .map_err(|source| SessionManagerError::File {
                operation: "open log",
                path: log_path.clone(),
                source,
            })?;

        let spawned = match self.inner.backend.spawn(&PtySpawnRequest {
            session_id: id,
            shell: shell.clone(),
            args: shell_launch.args,
            env: shell_launch.env,
            cwd: cwd.clone(),
            cols: request.cols,
            rows: request.rows,
        }) {
            Ok(spawned) => spawned,
            Err(error) => {
                drop(log_file);
                remove_staged_log(&log_path);
                return Err(error.into());
            }
        };
        let mut session = Session::new(
            id,
            self.inner.config.machine_id.clone(),
            shell,
            cwd.to_string_lossy(),
            (self.inner.config.clock)(),
        );
        session.project_id = project_id;
        session.origin = request.origin;
        session.capture = CaptureLevel::Full;
        session.title = request.title;
        session.pid = spawned.root_pid;
        session.continued_from = continued_from;
        if let Err(error) = session
            .validate()
            .and_then(|()| session.transition(SessionState::Idle, (self.inner.config.clock)()))
        {
            abort_spawn(&spawned, log_file, &log_path);
            return Err(error.into());
        }

        let (output, _) = broadcast::channel(256);
        let (structured, _) = broadcast::channel(256);
        let parser = Arc::new(Mutex::new(vt100::Parser::new(
            request.rows,
            request.cols,
            self.inner.config.scrollback_lines,
        )));
        let scrollback = Arc::new(Mutex::new(Scrollback::new(
            self.inner.config.scrollback_lines,
        )));
        let SpawnedPty {
            root_pid,
            reader,
            writer,
            resize,
            kill,
            try_wait,
        } = spawned;
        let handle = Arc::new(SessionHandle {
            id,
            root_pid,
            session: Mutex::new(session.clone()),
            writer,
            resize,
            kill,
            try_wait,
            pty_closed: AtomicBool::new(false),
            parser,
            scrollback,
            output,
            structured,
            observations: self.inner.observations.clone(),
            events: self.inner.events.clone(),
            tracker: Mutex::new(CommandTracker::new(
                self.inner.config.machine_id.clone(),
                cwd.to_string_lossy(),
                self.inner.tracker_ids.clone(),
            )),
            watcher: Mutex::new(
                watcher::EventEngine::new(None, self.inner.watcher_adapters.clone())
                    .with_command_activity_required(),
            ),
            line_framer: Mutex::new(watcher::LineFramer::default()),
            watcher_ids: self.inner.tracker_ids.clone(),
            initial_command: Mutex::new(initial_command),
            shell_reports: Mutex::default(),
            attached: Arc::new(AtomicBool::new(false)),
            log_tail: Mutex::new(None),
            adopted_tree: Mutex::default(),
        });
        Ok(PreparedSession {
            session,
            handle,
            reader,
            log_path,
            log_file,
            osc_nonce,
        })
    }

    fn publish(
        &self,
        prepared: PreparedSession,
        persistence: Persistence,
    ) -> Result<Session, SessionManagerError> {
        let id = prepared.session.id;
        let mut sessions = match self.inner.sessions.write() {
            Ok(sessions) => sessions,
            Err(_) => {
                rollback_prepared(prepared);
                return Err(SessionManagerError::Lock {
                    resource: "session map",
                    session_id: id,
                });
            }
        };
        if let Some(store) = &self.inner.store {
            let result = match persistence {
                Persistence::Ordinary => store.insert_session(&prepared.session),
                Persistence::Continuation(source_id) => {
                    store.insert_continuation(source_id, &prepared.session)
                }
            };
            if let Err(error) = result {
                drop(sessions);
                rollback_prepared(prepared);
                if let (
                    Persistence::Continuation(source_id),
                    store::StoreError::SuccessorExists { .. },
                ) = (persistence, &error)
                {
                    return store
                        .direct_successor(source_id)?
                        .ok_or(SessionManagerError::Store(error));
                }
                return Err(error.into());
            }
        }
        sessions.insert(id, prepared.handle.clone());
        drop(sessions);

        let try_wait = prepared.handle.try_wait.clone();
        tasks::spawn_reader(
            id,
            prepared.reader,
            prepared.handle.clone(),
            tasks::OutputLog {
                path: prepared.log_path,
                file: prepared.log_file,
            },
            OscParser::new(prepared.osc_nonce),
            self.inner.store.clone(),
            self.inner.config.clock.clone(),
        );
        tasks::spawn_exit_monitor(
            id,
            prepared.handle,
            try_wait,
            self.inner.config.clock.clone(),
            self.inner.store.clone(),
        );
        if prepared.session.origin == SessionOrigin::AutoWrap {
            self.spawn_auto_wrap_watchdog(id);
        }
        Ok(prepared.session)
    }

    fn spawn_auto_wrap_watchdog(&self, id: SessionId) {
        let manager = self.clone();
        tokio::spawn(async move {
            tokio::time::sleep(AUTO_WRAP_ATTACH_GRACE).await;
            if let Err(error) = manager.reap_unattached_auto_wrap(id) {
                tracing::warn!(session = id.0, %error, "failed to reap unattached AUTO_WRAP Session");
            }
        });
    }

    /// Kill an AUTO_WRAP Session that no terminal ever attached to.
    pub fn reap_unattached_auto_wrap(&self, id: SessionId) -> Result<bool, SessionManagerError> {
        let handle = self.handle(id)?;
        let session = lock_session(&handle)?.clone();
        if session.origin != SessionOrigin::AutoWrap
            || session.state.is_finished()
            || handle.attached.load(Ordering::SeqCst)
        {
            return Ok(false);
        }
        tracing::info!(session = id.0, "killing unattached AUTO_WRAP Session");
        self.kill(id)?;
        Ok(true)
    }
}

fn abort_spawn(spawned: &SpawnedPty, log_file: File, log_path: &PathBuf) {
    if let Err(error) = (spawned.kill)() {
        tracing::warn!(%error, "failed to kill rejected staged PTY");
    }
    drop(log_file);
    remove_staged_log(log_path);
}

fn rollback_prepared(prepared: PreparedSession) {
    if let Err(error) = (prepared.handle.kill)() {
        tracing::warn!(session = prepared.session.id.0, %error, "failed to kill rolled-back PTY");
    }
    drop(prepared.log_file);
    remove_staged_log(&prepared.log_path);
}

fn remove_staged_log(path: &PathBuf) {
    if let Err(error) = fs::remove_file(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(path = %path.display(), %error, "failed to remove staged Session log");
        }
    }
}

/// Render a Watch initial Command, or reject it before anything is spawned.
fn initial_command(
    origin: SessionOrigin,
    shell: &str,
    argv: Option<&[String]>,
) -> Result<Option<Vec<u8>>, SessionManagerError> {
    let Some(argv) = argv else {
        return Ok(None);
    };
    if origin != SessionOrigin::Watch {
        return Err(SessionManagerError::InitialCommandRequiresWatch { origin });
    }
    let dialect = ShellDialect::from_shell_path(shell).ok_or_else(|| {
        SessionManagerError::InitialCommandShell {
            shell: shell.to_string(),
        }
    })?;
    let mut line = render_command_line(argv, dialect)?.into_bytes();
    line.push(b'\r');
    Ok(Some(line))
}
