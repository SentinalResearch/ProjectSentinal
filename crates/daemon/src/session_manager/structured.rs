//! Persisted command queries and structured metric/event publication.

use super::{lock_session, tasks, SessionManager, SessionManagerError};
use protocol::{
    CaptureLevel, Command, DaemonMessage, Event, EventId, EventKind, MachineMetrics, Process,
    Resource, SessionId, SessionMetrics, Timestamp,
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

    /// Return a bounded persisted Event page.
    pub fn events(
        &self,
        session_id: Option<SessionId>,
        after: Option<EventId>,
        limit: u32,
    ) -> Result<Vec<Event>, SessionManagerError> {
        self.store()?
            .events(session_id, after, limit)
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
                    // An adopted root that exited is sampled through a survivor.
                    let sampled = handle
                        .adopted_tree
                        .lock()
                        .ok()
                        .and_then(|tree| tree.sampling_root)
                        .unwrap_or(pid);
                    roots.push((handle.id, sampled));
                }
            }
        }
        Ok(roots)
    }

    /// Feed one Session process tree into its deterministic Event engine.
    pub fn publish_processes(
        &self,
        session_id: SessionId,
        at: Timestamp,
        processes: Vec<Process>,
    ) -> Result<(), SessionManagerError> {
        let handle = self.handle(session_id)?;
        if lock_session(&handle)?.origin == protocol::SessionOrigin::Adopted {
            self.record_descendants(&handle, &processes)?;
        }
        tasks::apply_observation(
            session_id,
            &handle,
            self.inner.store.as_deref(),
            watcher::Observation {
                at,
                kind: watcher::ObservationKind::ProcessSample(watcher::ProcessObservation {
                    processes,
                }),
            },
        );
        Ok(())
    }

    /// Persist and broadcast current metrics to clients attached to the Session.
    pub fn publish_metrics(&self, metrics: SessionMetrics) -> Result<(), SessionManagerError> {
        let observation = watcher::MetricsObservation {
            cpu_percent: metrics.totals.cpu_percent,
            cpu_machine_percent: None,
            gpu_percent: None,
            ram_percent: None,
            vram_percent: None,
        };
        self.publish_metrics_with_resources(metrics, observation)
    }

    /// Persist metrics and feed complete resource percentages to the Event engine.
    pub fn publish_metrics_with_resources(
        &self,
        metrics: SessionMetrics,
        resources: watcher::MetricsObservation,
    ) -> Result<(), SessionManagerError> {
        let session_id = metrics.session_id;
        let handle = self.handle(session_id)?;
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
        let observation = watcher::Observation {
            at: metrics.at,
            kind: watcher::ObservationKind::MetricsSample(resources),
        };
        let message = DaemonMessage::SessionMetrics { metrics };
        if reduced {
            self.observe_reduced(message.clone());
        }
        let _ = handle.structured.send(message);
        tasks::apply_observation(
            session_id,
            &handle,
            self.inner.store.as_deref(),
            observation,
        );
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
        let _ = handle.events.send(event.clone());
        let message = DaemonMessage::Event {
            event: event.clone(),
        };
        let _ = handle.structured.send(message.clone());
        if session.capture != CaptureLevel::Full {
            self.observe_reduced(message);
        }
        tasks::apply_health_event(
            session_id,
            &handle,
            self.inner.store.as_deref(),
            at,
            &event.kind,
        );
        Ok(event)
    }

    fn store(&self) -> Result<&Store, SessionManagerError> {
        self.inner
            .store
            .as_deref()
            .ok_or(SessionManagerError::StoreUnavailable)
    }
}
