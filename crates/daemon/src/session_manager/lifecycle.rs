//! Session creation and PTY task startup.

use super::{
    tasks, validate_size, CreateSession, SessionHandle, SessionManager, SessionManagerError,
};
use crate::{
    pty::{PtySpawnRequest, SpawnedPty},
    scrollback::Scrollback,
    shell::{install_hooks, launch, ShellLaunch},
    tracker::CommandTracker,
};
use protocol::{Session, SessionId, SessionState};
use std::{
    fs::{self, OpenOptions},
    path::PathBuf,
    sync::{atomic::Ordering, Arc, Mutex},
};
use tokio::sync::broadcast;

impl SessionManager {
    /// Spawn a shell and return its session in the idle state.
    pub async fn create(&self, request: CreateSession) -> Result<Session, SessionManagerError> {
        validate_size(request.cols, request.rows)?;
        if request.origin != protocol::SessionOrigin::Ui || request.command.is_some() {
            return Err(SessionManagerError::WatchUnavailable {
                origin: request.origin,
            });
        }
        let id = SessionId(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let shell = request
            .shell
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(|| "/bin/sh".into());
        let cwd = request
            .cwd
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("/"));
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.clone());
        let shell_launch = match install_hooks(&self.inner.config.data_dir, &home) {
            Ok(hook_dir) => launch(PathBuf::from(&shell).as_path(), &hook_dir),
            Err(error) => {
                tracing::warn!(%error, "shell hooks unavailable; starting Session without command tracking");
                ShellLaunch::default()
            }
        };
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
            args: shell_launch.args,
            env: shell_launch.env,
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
        session.origin = request.origin;
        session.capture = protocol::CaptureLevel::Full;
        session.title = request.title;
        session.pid = spawned.root_pid;
        session.validate()?;
        session.transition(SessionState::Idle, (self.inner.config.clock)())?;

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
            parser: parser.clone(),
            scrollback: scrollback.clone(),
            output: output.clone(),
            structured,
            tracker: Mutex::new(CommandTracker::new(
                self.inner.config.machine_id.clone(),
                cwd.to_string_lossy(),
                self.inner.tracker_ids.clone(),
            )),
        });
        self.inner
            .sessions
            .write()
            .map_err(|_| SessionManagerError::Lock {
                resource: "session map",
                session_id: id,
            })?
            .insert(id, handle.clone());

        if let Some(store) = &self.inner.store {
            store.insert_session(&session)?;
        }

        tasks::spawn_reader(
            id,
            reader,
            handle.clone(),
            log_path,
            log_file,
            self.inner.store.clone(),
            self.inner.config.clock.clone(),
        );
        tasks::spawn_exit_monitor(
            id,
            handle,
            try_wait,
            self.inner.config.clock.clone(),
            self.inner.store.clone(),
        );
        Ok(session)
    }
}
