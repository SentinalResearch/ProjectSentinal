//! Viewers and the one size of a Session (T3): attach, reports, typing, and
//! applying the size the policy in [`super::viewers`] settles on.

use super::capture::{require_capture, CaptureOperation};
use super::operations::snapshot_attachment;
use super::viewers::{SizeChange, ViewerId, SETTLE_MS};
use super::{
    validate_size, Attachment, SessionHandle, SessionManager, SessionManagerError, ViewerHandle,
    ViewerOptions,
};
use crate::audit::{AuditRecord, AuditResize};
use protocol::SessionId;
use std::sync::{atomic::Ordering, Arc};

impl SessionManager {
    /// Attach a viewer: register it, let it take the size if it is alone,
    /// then snapshot the screen at the Session's size.
    pub fn attach_viewer(
        &self,
        session_id: SessionId,
        history: bool,
        options: ViewerOptions,
    ) -> Result<Attachment, SessionManagerError> {
        let handle = self.handle(session_id)?;
        require_capture(&handle, CaptureOperation::Attach)?;
        if let Some((cols, rows)) = options.size {
            validate_size(cols, rows)?;
        }
        let id = ViewerId(self.inner.next_viewer.fetch_add(1, Ordering::Relaxed));
        let taken =
            lock_viewers(&handle)?.attach(id, options.size, options.mode, &options.client_addr);
        if let Some(change) = taken {
            // Resized before the snapshot, which the screen thread then
            // takes at the new size.
            self.apply_size(&handle, change, &options.client_addr)?;
        }
        let viewer = ViewerHandle {
            id,
            sized: options.size.is_some(),
            size: handle.size.subscribe(),
            handle: handle.clone(),
        };
        let mut attachment = snapshot_attachment(&handle, history);
        attachment.viewer = Some(viewer);
        Ok(attachment)
    }

    /// `VIEWER_STATE` from an attached viewer.
    pub fn viewer_state(
        &self,
        session_id: SessionId,
        viewer: ViewerId,
        cols: u16,
        rows: u16,
        active: bool,
    ) -> Result<(), SessionManagerError> {
        validate_size(cols, rows)?;
        let handle = self.operation_handle(session_id, CaptureOperation::Resize)?;
        let now = (self.inner.config.clock)().0;
        let due = {
            let mut viewers = lock_viewers(&handle)?;
            viewers.report(viewer, cols, rows, active, now);
            viewers.due_at()
        };
        self.settle_later(session_id, due);
        Ok(())
    }

    /// Whether this viewer may type; an observer may not.
    pub fn viewer_may_type(
        &self,
        session_id: SessionId,
        viewer: ViewerId,
    ) -> Result<bool, SessionManagerError> {
        let handle = self.handle(session_id)?;
        let may = lock_viewers(&handle)?.may_type(viewer);
        Ok(may)
    }

    /// The viewer typed: it becomes the one that sets the size.
    pub fn viewer_input(
        &self,
        session_id: SessionId,
        viewer: ViewerId,
    ) -> Result<(), SessionManagerError> {
        let handle = self.handle(session_id)?;
        let now = (self.inner.config.clock)().0;
        let due = {
            let mut viewers = lock_viewers(&handle)?;
            viewers.input(viewer, now);
            viewers.due_at()
        };
        self.settle_later(session_id, due);
        Ok(())
    }

    /// An old client's `RESIZE`: the size at once, from `viewer` when the
    /// connection is attached.
    pub fn legacy_resize(
        &self,
        session_id: SessionId,
        viewer: Option<ViewerId>,
        cols: u16,
        rows: u16,
        client_addr: &str,
    ) -> Result<(), SessionManagerError> {
        validate_size(cols, rows)?;
        let handle = self.operation_handle(session_id, CaptureOperation::Resize)?;
        let change = lock_viewers(&handle)?.legacy_resize(viewer, cols, rows);
        if let Some(change) = change {
            self.apply_size(&handle, change, client_addr)?;
        }
        Ok(())
    }

    /// The Session's size now (T3).
    pub fn session_size(&self, session_id: SessionId) -> Result<(u16, u16), SessionManagerError> {
        let handle = self.handle(session_id)?;
        let size = *handle.size.borrow();
        Ok((size.cols, size.rows))
    }

    /// Apply a settled size when it is due: a timer per report, so a newer
    /// report's timer is the one that applies it.
    fn settle_later(&self, session_id: SessionId, due: Option<i64>) {
        let Some(due) = due else {
            return;
        };
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let manager = self.clone();
        runtime.spawn(async move {
            let wait = u64::try_from(SETTLE_MS).unwrap_or(150);
            tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
            if let Err(error) = manager.settle_now(session_id, due) {
                tracing::debug!(session = session_id.0, %error, "size not applied");
            }
        });
    }

    /// Apply the settled size, if the report due at `due` is still the
    /// newest. The timer has waited the settle time, so `due` counts as now
    /// even under a frozen test clock.
    fn settle_now(&self, session_id: SessionId, due: i64) -> Result<(), SessionManagerError> {
        let handle = self.handle(session_id)?;
        let now = (self.inner.config.clock)().0.max(due);
        let (change, client_addr) = {
            let mut viewers = lock_viewers(&handle)?;
            if viewers.due_at() != Some(due) {
                return Ok(());
            }
            let change = viewers.settle(now);
            let client_addr = change
                .and_then(|change| change.owner)
                .and_then(|owner| viewers.address(owner))
                .unwrap_or_else(|| "local".into());
            (change, client_addr)
        };
        if let Some(change) = change {
            self.apply_size(&handle, change, &client_addr)?;
        }
        Ok(())
    }

    /// Resize the PTY and the screen, audit it, then tell every viewer.
    fn apply_size(
        &self,
        handle: &Arc<SessionHandle>,
        change: SizeChange,
        client_addr: &str,
    ) -> Result<(), SessionManagerError> {
        self.inner.audit.append(&AuditRecord {
            timestamp: (self.inner.config.clock)(),
            client_addr: client_addr.to_string(),
            session_id: handle.id,
            byte_count: 0,
            resize: Some(AuditResize {
                cols: change.cols,
                rows: change.rows,
            }),
        })?;
        if let Err(error) = (handle.resize)(change.cols, change.rows) {
            // A Session whose program has exited keeps its last size.
            tracing::debug!(session = handle.id.0, %error, "PTY not resized");
        }
        handle.screen.resize(change.cols, change.rows);
        handle.size.send_replace(change);
        Ok(())
    }
}

fn lock_viewers(
    handle: &SessionHandle,
) -> Result<std::sync::MutexGuard<'_, super::viewers::Viewers>, SessionManagerError> {
    handle
        .viewers
        .lock()
        .map_err(|_| SessionManagerError::Lock {
            resource: "viewers",
            session_id: handle.id,
        })
}
