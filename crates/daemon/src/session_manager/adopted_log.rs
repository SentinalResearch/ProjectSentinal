//! Read-only log tails and descendant liveness for adopted processes.
//!
//! A tail is owned by its Session handle so the background poller and the
//! process-ended path can never read the same bytes twice or out of order.
//! Log lines enter the same watcher pipeline as PTY output; they are never
//! Commands and never instructions. Tail problems become structured warning
//! Events, and a tail that cannot continue downgrades capture to `PROCESS`.

use super::{lock_session, tasks, SessionHandle, SessionManager, SessionManagerError};
use crate::log_tail::{LogCursor, LogTail, LogTailChunk, LogTailNotice, LogTailPolicy};
use protocol::{
    CaptureLevel, DaemonMessage, Event, EventKind, Process, SessionId, SessionLog, Timestamp,
};
use std::{collections::BTreeMap, time::Duration};
use store::StoredLogCursor;

const LOG_TAIL_POLL: Duration = Duration::from_millis(50);
/// A restart may replay at most this much recently read log time.
const CURSOR_SAVE_INTERVAL_MS: i64 = 1_000;
/// Remembered descendants per adopted Session.
const MAX_DESCENDANTS: usize = 256;

/// A running tail plus the bookkeeping needed to resume it.
pub(super) struct ActiveLogTail {
    tail: LogTail,
    path: String,
    saved_at: Option<Timestamp>,
}

/// Descendants seen under an adopted root, by PID and OS start time.
#[derive(Default)]
pub(super) struct AdoptedTree {
    descendants: BTreeMap<u32, Timestamp>,
    /// A surviving descendant sampled for Metrics once the root has exited.
    pub(super) sampling_root: Option<u32>,
}

/// Open a tail for `log`, resuming from `cursor` when it still matches.
pub(super) fn open_tail(
    log: &SessionLog,
    owner_uid: u32,
    cursor: Option<LogCursor>,
) -> Result<LogTail, SessionManagerError> {
    LogTail::open(
        log.path.clone().into(),
        owner_uid,
        LogTailPolicy::default(),
        cursor,
    )
    .map_err(|error| SessionManagerError::InvalidLog {
        path: log.path.clone(),
        reason: error.to_string(),
    })
}

impl SessionManager {
    /// Open `log` for Session `id`, resuming from its persisted cursor.
    pub(super) fn open_session_tail(
        &self,
        id: SessionId,
        log: &SessionLog,
    ) -> Result<LogTail, SessionManagerError> {
        let owner_uid = self
            .inner
            .probe
            .owner_uid()
            .ok_or(SessionManagerError::ProcessProbeUnavailable)?;
        let cursor = match &self.inner.store {
            Some(store) => store
                .log_cursor(id)?
                .filter(|cursor| cursor.path == log.path)
                .map(|cursor| LogCursor {
                    device: cursor.device,
                    inode: cursor.inode,
                    offset: cursor.offset,
                }),
            None => None,
        };
        open_tail(log, owner_uid, cursor)
    }

    /// Upgrade Session `id` to `PROCESS_LOG` and start reading `tail`.
    pub(super) fn activate_log_tail(
        &self,
        id: SessionId,
        tail: LogTail,
        log: &SessionLog,
    ) -> Result<(), SessionManagerError> {
        let handle = self.handle(id)?;
        *lock_tail(&handle)? = Some(ActiveLogTail {
            tail,
            path: log.path.clone(),
            saved_at: None,
        });
        self.set_capture(&handle, CaptureLevel::ProcessLog)?;
        self.spawn_log_tail(id);
        Ok(())
    }

    /// Read one bounded chunk of Session `id`'s log; `false` once no tail is active.
    ///
    /// The daemon calls this on a timer; tests call it directly instead.
    pub fn poll_log_tail(&self, id: SessionId) -> Result<bool, SessionManagerError> {
        let handle = self.handle(id)?;
        let mut guard = lock_tail(&handle)?;
        let Some(active) = guard.as_mut() else {
            return Ok(false);
        };
        match active.tail.poll() {
            Ok(chunk) => {
                let now = (self.inner.config.clock)();
                self.apply_tail_chunk(&handle, chunk, now);
                self.save_cursor(id, active, now);
                Ok(true)
            }
            Err(error) => {
                *guard = None;
                drop(guard);
                self.stop_log_tail(&handle, &error.to_string())?;
                Ok(false)
            }
        }
    }

    /// Drain and close the tail after the adopted process tree has ended.
    pub(super) fn finish_log_tail(
        &self,
        handle: &SessionHandle,
    ) -> Result<(), SessionManagerError> {
        let Some(mut active) = lock_tail(handle)?.take() else {
            return Ok(());
        };
        let now = (self.inner.config.clock)();
        match active.tail.drain() {
            Ok(chunk) => self.apply_tail_chunk(handle, chunk, now),
            Err(error) => self.publish_tail_warning(
                handle,
                &format!("Sentinal could not read the final log output: {error}"),
            )?,
        }
        tasks::finish_output(handle.id, handle, self.inner.store.as_deref(), now);
        if let Some(store) = &self.inner.store {
            store.delete_log_cursor(handle.id)?;
        }
        Ok(())
    }

    /// Record a structured warning for a log that cannot be tailed at all.
    pub(super) fn warn_log_unavailable(
        &self,
        id: SessionId,
        log: &SessionLog,
        error: &SessionManagerError,
    ) -> Result<(), SessionManagerError> {
        let handle = self.handle(id)?;
        tracing::warn!(session = id.0, %error, "adopted log is not tailable");
        self.publish_tail_warning(
            &handle,
            &format!("Sentinal cannot read the log {}: {error}", log.path),
        )
    }

    /// Remember the adopted root's current descendants from a process sample.
    pub(super) fn record_descendants(
        &self,
        handle: &SessionHandle,
        processes: &[Process],
    ) -> Result<(), SessionManagerError> {
        let Some(root) = handle.root_pid else {
            return Ok(());
        };
        let owner = self.inner.probe.owner_uid();
        let mut tree = lock_tree(handle)?;
        let sampled_root = tree.sampling_root.unwrap_or(root);
        if !processes.iter().any(|process| process.pid == sampled_root) {
            return Ok(());
        }
        for process in processes {
            if process.pid == root
                || tree.descendants.contains_key(&process.pid)
                || tree.descendants.len() >= MAX_DESCENDANTS
            {
                continue;
            }
            if let Some(identity) = self.inner.probe.identify(process.pid) {
                if Some(identity.uid) == owner {
                    tree.descendants.insert(process.pid, identity.started_at);
                }
            }
        }
        Ok(())
    }

    /// Whether a remembered descendant of an exited adopted root still runs.
    ///
    /// Dead descendants are forgotten; the first survivor becomes the Metrics root.
    pub(super) fn adopted_descendant_alive(
        &self,
        handle: &SessionHandle,
    ) -> Result<bool, SessionManagerError> {
        let owner = self.inner.probe.owner_uid();
        let mut tree = lock_tree(handle)?;
        let probe = &self.inner.probe;
        tree.descendants.retain(|pid, started_at| {
            probe.identify(*pid).is_some_and(|identity| {
                Some(identity.uid) == owner && identity.started_at == *started_at
            })
        });
        tree.sampling_root = tree.descendants.keys().next().copied();
        Ok(tree.sampling_root.is_some())
    }

    fn apply_tail_chunk(&self, handle: &SessionHandle, chunk: LogTailChunk, at: Timestamp) {
        if chunk.reset {
            match handle.line_framer.lock() {
                Ok(mut framer) => framer.reset(),
                Err(_) => tracing::error!(session = handle.id.0, "line-framer lock poisoned"),
            }
        }
        for notice in chunk.notices {
            match notice {
                LogTailNotice::BacklogSkipped { skipped_bytes } => {
                    let message =
                        format!("Sentinal skipped {skipped_bytes} bytes of log output to keep up");
                    if let Err(error) = self.publish_tail_warning(handle, &message) {
                        tracing::warn!(session = handle.id.0, %error, "failed to record log warning");
                    }
                }
                other => tracing::debug!(session = handle.id.0, notice = ?other, "adopted log"),
            }
        }
        if !chunk.bytes.is_empty() {
            tasks::apply_output(
                handle.id,
                handle,
                self.inner.store.as_deref(),
                &chunk.bytes,
                at,
            );
        }
    }

    fn save_cursor(&self, id: SessionId, active: &mut ActiveLogTail, now: Timestamp) {
        let Some(store) = &self.inner.store else {
            return;
        };
        if active
            .saved_at
            .is_some_and(|saved| now.0.saturating_sub(saved.0) < CURSOR_SAVE_INTERVAL_MS)
        {
            return;
        }
        let cursor = active.tail.cursor();
        let stored = StoredLogCursor {
            path: active.path.clone(),
            device: cursor.device,
            inode: cursor.inode,
            offset: cursor.offset,
        };
        match store.save_log_cursor(id, &stored) {
            Ok(()) => active.saved_at = Some(now),
            Err(error) => tracing::warn!(session = id.0, %error, "failed to save log cursor"),
        }
    }

    /// Fall back to `PROCESS` with a structured warning.
    fn stop_log_tail(
        &self,
        handle: &SessionHandle,
        reason: &str,
    ) -> Result<(), SessionManagerError> {
        match handle.line_framer.lock() {
            Ok(mut framer) => framer.reset(),
            Err(_) => tracing::error!(session = handle.id.0, "line-framer lock poisoned"),
        }
        if let Some(store) = &self.inner.store {
            store.delete_log_cursor(handle.id)?;
        }
        if !lock_session(handle)?.state.is_finished() {
            self.set_capture(handle, CaptureLevel::Process)?;
        }
        self.publish_tail_warning(
            handle,
            &format!("Sentinal stopped reading the log: {reason}"),
        )
    }

    fn set_capture(
        &self,
        handle: &SessionHandle,
        capture: CaptureLevel,
    ) -> Result<(), SessionManagerError> {
        let session = {
            let mut session = lock_session(handle)?;
            session.capture = capture;
            session.validate()?;
            session.clone()
        };
        match handle.watcher.lock() {
            Ok(mut engine) => engine.set_capture(capture),
            Err(_) => tracing::error!(session = handle.id.0, "watcher engine lock poisoned"),
        }
        if let Some(store) = &self.inner.store {
            store.update_session(&session)?;
        }
        let message = DaemonMessage::SessionUpdated { session };
        self.observe_reduced(message.clone());
        let _ = handle.structured.send(message);
        Ok(())
    }

    fn publish_tail_warning(
        &self,
        handle: &SessionHandle,
        message: &str,
    ) -> Result<(), SessionManagerError> {
        let at = (self.inner.config.clock)();
        let event = Event {
            id: self.inner.tracker_ids.event(),
            session_id: handle.id,
            command_id: None,
            at,
            kind: EventKind::WarningDetected {
                message: message.to_string(),
            },
        };
        if let Some(store) = &self.inner.store {
            store.insert_event(&event)?;
        }
        let kind = event.kind.clone();
        let _ = handle.events.send(event.clone());
        let message = DaemonMessage::Event { event };
        let _ = handle.structured.send(message.clone());
        self.observe_reduced(message);
        tasks::apply_health_event(handle.id, handle, self.inner.store.as_deref(), at, &kind);
        Ok(())
    }

    fn spawn_log_tail(&self, id: SessionId) {
        if !self.inner.log_tail_polling {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            tracing::warn!(
                session = id.0,
                "cannot start adopted log tail without a Tokio runtime"
            );
            return;
        };
        let manager = self.clone();
        runtime.spawn(async move {
            loop {
                match manager.poll_log_tail(id) {
                    Ok(true) => tokio::time::sleep(LOG_TAIL_POLL).await,
                    Ok(false) => return,
                    Err(error) => {
                        tracing::warn!(session = id.0, %error, "adopted log tail stopped");
                        return;
                    }
                }
            }
        });
    }
}

fn lock_tail(
    handle: &SessionHandle,
) -> Result<std::sync::MutexGuard<'_, Option<ActiveLogTail>>, SessionManagerError> {
    handle
        .log_tail
        .lock()
        .map_err(|_| SessionManagerError::Lock {
            resource: "log tail",
            session_id: handle.id,
        })
}

fn lock_tree(
    handle: &SessionHandle,
) -> Result<std::sync::MutexGuard<'_, AdoptedTree>, SessionManagerError> {
    handle
        .adopted_tree
        .lock()
        .map_err(|_| SessionManagerError::Lock {
            resource: "adopted process tree",
            session_id: handle.id,
        })
}
