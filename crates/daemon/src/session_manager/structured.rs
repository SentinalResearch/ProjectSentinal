//! Persisted command queries and structured metric/event publication.

use super::{lock_session, SessionManager, SessionManagerError};
use protocol::{
    CaptureLevel, Command, DaemonMessage, Event, EventKind, MachineMetrics, Resource, SessionId,
    SessionMetrics, Timestamp,
};
use store::{RawMetricSample, Store};
use tokio::sync::broadcast;

impl SessionManager {
    /// Return recent persisted command history.
    pub fn history(
        &self,
        session_id: Option<SessionId>,
        limit: u32,
    ) -> Result<Vec<Command>, SessionManagerError> {
        self.store()?.history(session_id, limit).map_err(Into::into)
    }

    /// Search persisted command text through SQLite FTS5.
    pub fn search_commands(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<Command>, SessionManagerError> {
        self.store()?
            .search_commands(query, limit)
            .map_err(Into::into)
    }

    /// Return live Session root PIDs for process-tree aggregation.
    pub fn process_roots(&self) -> Result<Vec<(SessionId, u32)>, SessionManagerError> {
        let sessions = self
            .inner
            .sessions
            .read()
            .map_err(|_| SessionManagerError::Lock {
                resource: "session map",
                session_id: SessionId(0),
            })?;
        let mut roots = Vec::new();
        for handle in sessions.values() {
            if let Some(pid) = handle.root_pid {
                if !lock_session(handle)?.state.is_finished() {
                    roots.push((handle.id, pid));
                }
            }
        }
        Ok(roots)
    }

    /// Persist and broadcast current metrics to clients attached to the Session.
    pub fn publish_metrics(&self, metrics: SessionMetrics) -> Result<(), SessionManagerError> {
        let handle = self.handle(metrics.session_id)?;
        if let Some(store) = &self.inner.store {
            store.insert_raw_metric(&RawMetricSample {
                session_id: Some(metrics.session_id),
                at: metrics.at,
                cpu_percent: metrics.totals.cpu_percent,
                ram_used_bytes: metrics.totals.rss_bytes,
                ram_total_bytes: 0,
                gpu_percent: None,
                vram_used_bytes: Some(metrics.totals.gpu_mem_bytes),
                vram_total_bytes: None,
            })?;
        }
        let reduced = lock_session(&handle)?.capture != CaptureLevel::Full;
        let message = DaemonMessage::SessionMetrics { metrics };
        if reduced {
            self.observe_reduced(message.clone());
        }
        let _ = handle.structured.send(message);
        Ok(())
    }

    /// Subscribe to Metrics and state changes of reduced-capture Sessions.
    ///
    /// Every WebSocket client receives these without attaching: reading
    /// Metrics is observation, allowed for every capture level.
    pub fn observe(&self) -> broadcast::Receiver<DaemonMessage> {
        self.inner.observations.subscribe()
    }

    pub(super) fn observe_reduced(&self, message: DaemonMessage) {
        let _ = self.inner.observations.send(message);
    }

    /// Persist machine metrics and broadcast them to every connected client.
    pub fn publish_machine_metrics(
        &self,
        metrics: MachineMetrics,
    ) -> Result<(), SessionManagerError> {
        if let Some(store) = &self.inner.store {
            let has_gpus = !metrics.gpus.is_empty();
            let gpu_percent = metrics
                .gpus
                .iter()
                .map(|gpu| gpu.util_percent)
                .reduce(f32::max);
            store.insert_raw_metric(&RawMetricSample {
                session_id: None,
                at: metrics.at,
                cpu_percent: metrics.cpu_percent,
                ram_used_bytes: metrics.ram_used_bytes,
                ram_total_bytes: metrics.ram_total_bytes,
                gpu_percent,
                vram_used_bytes: has_gpus.then(|| {
                    metrics.gpus.iter().fold(0_u64, |total, gpu| {
                        total.saturating_add(gpu.vram_used_bytes)
                    })
                }),
                vram_total_bytes: has_gpus.then(|| {
                    metrics.gpus.iter().fold(0_u64, |total, gpu| {
                        total.saturating_add(gpu.vram_total_bytes)
                    })
                }),
            })?;
        }
        let _ = self
            .inner
            .observations
            .send(DaemonMessage::MachineMetrics { metrics });
        Ok(())
    }

    /// Persist and broadcast a debounced high-resource Event.
    pub fn publish_resource_high(
        &self,
        session_id: SessionId,
        resource: Resource,
        percent: f32,
        at: Timestamp,
    ) -> Result<Event, SessionManagerError> {
        let handle = self.handle(session_id)?;
        let session = lock_session(&handle)?.clone();
        let command_id = session.current_command;
        let event = Event {
            id: self.inner.tracker_ids.event(),
            session_id,
            command_id,
            at,
            kind: EventKind::ResourceHigh { resource, percent },
        };
        if let Some(store) = &self.inner.store {
            store.insert_event(&event)?;
        }
        let message = DaemonMessage::Event {
            event: event.clone(),
        };
        let _ = handle.structured.send(message.clone());
        if session.capture != CaptureLevel::Full {
            self.observe_reduced(message);
        }
        Ok(event)
    }

    fn store(&self) -> Result<&Store, SessionManagerError> {
        self.inner
            .store
            .as_deref()
            .ok_or(SessionManagerError::StoreUnavailable)
    }
}
