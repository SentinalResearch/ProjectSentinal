//! An agent's own conversation file, followed as chat and progress (AC1).
//!
//! Design: `docs/AGENT_CHAT_DESIGN.md`. The agent names its file from its
//! start hook (`sentinal agent-chat link`); the daemon accepts it only from a
//! process running inside that Session, only for a regular file owned by the
//! daemon's user under `~/.claude/projects/`. The file is only read. Entries
//! are kept in memory (at most [`MAX_CHAT_ENTRIES`]) and sent to the
//! Session's viewers; nothing is stored. Progress becomes an agent report, so
//! the task, step N of M and Sentinal's time estimate show where 6.5's do.

use super::{agent_state, lock_session, SessionHandle, SessionManager, SessionManagerError};
use crate::log_tail::{LogTail, LogTailPolicy};
use protocol::agent_chat::MAX_CHAT_ENTRIES;
use protocol::{AgentStatus, CaptureLevel, ChatEntry, DaemonMessage, Session, SessionId};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Weak},
    time::Duration,
};
use watcher::agent_transcript::{TranscriptProgress, TranscriptReader};

/// How often the file is read.
const POLL: Duration = Duration::from_millis(500);
/// Most of an existing file read when it is linked: its recent past.
const INITIAL_BYTES: u64 = 4 * 1024 * 1024;
/// Unreadable lines tolerated before the chat is given up as unreadable.
const MAX_UNREADABLE: u32 = 50;

/// One Session's linked file and kept chat.
#[derive(Debug, Default)]
pub(crate) struct ChatState {
    path: Option<PathBuf>,
    /// Bumped on every link, so an older follower stops.
    generation: u64,
    entries: VecDeque<ChatEntry>,
}

impl SessionManager {
    /// Link the conversation file an agent inside `session_id` named.
    pub(crate) fn link_agent_transcript(
        &self,
        session_id: SessionId,
        path: &str,
        caller_pid: Option<u32>,
    ) -> Result<Session, SessionManagerError> {
        let caller = caller_pid.ok_or(SessionManagerError::AgentCallerUnknown)?;
        let handle = self.handle(session_id)?;
        let root = {
            let session = lock_session(&handle)?;
            if session.capture != CaptureLevel::Full || session.state.is_finished() {
                return Err(SessionManagerError::AgentReportsNotTaken(session_id));
            }
            session.pid
        };
        if !root.is_some_and(|root| self.inner.probe.descends_from(caller, root)) {
            return Err(SessionManagerError::AgentOutsideSession {
                pid: caller,
                session_id,
            });
        }
        let path = allowed_path(Path::new(path)).inspect_err(|error| {
            tracing::warn!(session = session_id.0, %error, "agent conversation file refused");
        })?;
        tracing::info!(session = session_id.0, "agent conversation linked");
        let owner = self
            .inner
            .probe
            .owner_uid()
            .ok_or(SessionManagerError::ProcessProbeUnavailable)?;
        let generation = {
            let mut chat = lock_chat(&handle)?;
            chat.generation += 1;
            chat.path = Some(path.clone());
            chat.entries.clear();
            chat.generation
        };
        let _ = handle
            .structured
            .send(chat_message(session_id, true, true, Vec::new()));
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let weak = Arc::downgrade(&handle);
            let clock = self.inner.config.clock.clone();
            runtime.spawn(follow(weak, path, owner, generation, clock));
        }
        let session = lock_session(&handle)?.clone();
        Ok(session)
    }

    /// `sentinal detach` from inside `session_id`: its attached terminals let
    /// go, as with the detach key. Only a process inside the Session may ask,
    /// so another program cannot drop someone's terminal.
    pub(crate) fn request_detach(
        &self,
        session_id: SessionId,
        caller_pid: Option<u32>,
    ) -> Result<Session, SessionManagerError> {
        let caller = caller_pid.ok_or(SessionManagerError::AgentCallerUnknown)?;
        let handle = self.handle(session_id)?;
        let root = lock_session(&handle)?.pid;
        if !root.is_some_and(|root| self.inner.probe.descends_from(caller, root)) {
            return Err(SessionManagerError::AgentOutsideSession {
                pid: caller,
                session_id,
            });
        }
        let _ = handle
            .structured
            .send(DaemonMessage::DetachRequested { session_id });
        let session = lock_session(&handle)?.clone();
        Ok(session)
    }

    /// The Session's kept chat, and whether a file is linked.
    pub fn chat(
        &self,
        session_id: SessionId,
    ) -> Result<(bool, Vec<ChatEntry>), SessionManagerError> {
        let handle = self.handle(session_id)?;
        let chat = lock_chat(&handle)?;
        Ok((chat.path.is_some(), chat.entries.iter().cloned().collect()))
    }
}

/// Only a file in Claude Code's own conversation folder. The file itself may
/// not exist yet (Claude Code writes it with the first message), so its
/// folder is resolved here; the file is checked when it is opened.
fn allowed_path(path: &Path) -> Result<PathBuf, SessionManagerError> {
    let refused = |reason| SessionManagerError::TranscriptRefused { reason };
    let home = std::env::var_os("HOME").ok_or(refused("no home folder"))?;
    let projects = Path::new(&home)
        .join(".claude")
        .join("projects")
        .canonicalize()
        .map_err(|_| refused("no Claude Code conversation folder"))?;
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(refused("not a file path"));
    };
    let folder = parent
        .canonicalize()
        .map_err(|_| refused("no such folder"))?;
    // A symlink anywhere on the way would make these differ.
    if folder != parent || !folder.starts_with(&projects) {
        return Err(refused("not in Claude Code's conversation folder"));
    }
    Ok(folder.join(name))
}

fn lock_chat(
    handle: &SessionHandle,
) -> Result<std::sync::MutexGuard<'_, ChatState>, SessionManagerError> {
    handle.chat.lock().map_err(|_| SessionManagerError::Lock {
        resource: "agent chat",
        session_id: handle.id,
    })
}

fn chat_message(
    session_id: SessionId,
    linked: bool,
    from_start: bool,
    entries: Vec<ChatEntry>,
) -> DaemonMessage {
    DaemonMessage::Chat {
        session_id,
        linked,
        from_start,
        entries,
    }
}

/// Read the file as it grows until the Session ends or another file is linked.
async fn follow(
    weak: Weak<SessionHandle>,
    path: PathBuf,
    owner: u32,
    generation: u64,
    clock: super::Clock,
) {
    let policy = LogTailPolicy {
        read_bytes: 256 * 1024,
        initial_tail_bytes: INITIAL_BYTES,
        max_backlog_bytes: 16 * 1024 * 1024,
    };
    let mut tail: Option<LogTail> = None;
    let mut reader = TranscriptReader::default();
    let mut partial: Vec<u8> = Vec::new();
    loop {
        let Some(handle) = weak.upgrade() else { return };
        let current = handle
            .chat
            .lock()
            .is_ok_and(|chat| chat.generation == generation);
        let live = handle
            .session
            .lock()
            .is_ok_and(|session| !session.state.is_finished());
        if !current || !live {
            return;
        }
        if tail.is_none() {
            // Opening checks the owner and that it is a regular file, not a
            // link. Until Claude Code writes it, there is nothing to open.
            match LogTail::open(path.clone(), owner, policy, None) {
                Ok(opened) => tail = Some(opened),
                Err(error) if path.exists() => {
                    tracing::warn!(session = handle.id.0, %error, "agent conversation file refused");
                    unlink(&handle, generation);
                    return;
                }
                Err(_) => {
                    drop(handle);
                    tokio::time::sleep(POLL).await;
                    continue;
                }
            }
        }
        let Some(open) = tail.as_mut() else { continue };
        let chunk = match open.poll() {
            Ok(chunk) => chunk,
            Err(error) => {
                tracing::warn!(session = handle.id.0, %error, "agent conversation file unreadable");
                unlink(&handle, generation);
                return;
            }
        };
        if chunk.reset {
            partial.clear();
        }
        partial.extend_from_slice(&chunk.bytes);
        let mut entries = Vec::new();
        let mut progress: Option<TranscriptProgress> = None;
        while let Some(end) = partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = partial.drain(..=end).collect();
            let update = reader.line(String::from_utf8_lossy(&line).trim_end());
            entries.extend(update.entries);
            if update.progress.is_some() {
                progress = update.progress;
            }
        }
        let (skipped, _) = reader.unreadable();
        if skipped > MAX_UNREADABLE {
            tracing::warn!(
                session = handle.id.0,
                "agent conversation format not readable"
            );
            unlink(&handle, generation);
            return;
        }
        if !entries.is_empty() {
            if let Ok(mut chat) = handle.chat.lock() {
                chat.entries.extend(entries.iter().cloned());
                while chat.entries.len() > MAX_CHAT_ENTRIES {
                    chat.entries.pop_front();
                }
            }
            let _ = handle
                .structured
                .send(chat_message(handle.id, true, false, entries));
        }
        if let Some(progress) = progress {
            let report = agent_state::AgentReport {
                status: Some(if progress.working {
                    AgentStatus::Working
                } else {
                    AgentStatus::Done
                }),
                detail: progress.detail,
                task: progress.task,
                step: progress.step,
                steps: progress.steps,
                eta_secs: None,
            };
            if let Err(error) = agent_state::submit(&handle, Some(&handle), report, clock()) {
                tracing::debug!(session = handle.id.0, %error, "agent progress not applied");
            }
        }
        drop(handle);
        tokio::time::sleep(POLL).await;
    }
}

/// Stop showing a chat that can no longer be read; the terminal stays.
fn unlink(handle: &SessionHandle, generation: u64) {
    if let Ok(mut chat) = handle.chat.lock() {
        if chat.generation != generation {
            return;
        }
        chat.path = None;
        chat.entries.clear();
    }
    let _ = handle
        .structured
        .send(chat_message(handle.id, false, true, Vec::new()));
}
