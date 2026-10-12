//! Local structured-state recorder for the durable projection outbox.

use crate::{control::ControlService, link::projection_epoch, session_manager::SessionManager};
use protocol::{
    DaemonMessage, MachineId, MachineMetrics, ProjectedEventSummary, ProjectedMetricScope,
    ProjectedMetrics, ProjectedSession, ProjectionBatch, ProjectionChange, SessionMetrics,
    Timestamp,
};
use std::{collections::HashMap, sync::Arc, time::Duration};
use store::{ProjectionOutboxLimits, Store};

/// Record Session, Event, and coalescible Metrics summaries outside hot paths.
pub fn spawn_projection_recorder(
    sessions: SessionManager,
    store: Arc<Store>,
    machine_id: MachineId,
    control: Option<ControlService>,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let epoch = projection_epoch(&machine_id);
        let mut events = sessions.subscribe_events();
        let mut observations = sessions.observe_projection();
        // Session create/kill/continue have no dedicated broadcast into this
        // recorder (unlike Metrics, which pushes through `observe_projection`);
        // this scan is how they reach the projection outbox at all. A newly
        // created Session must be visible to an owner who never attached to
        // it — including one who never had the browser open — well under a
        // second, not the old 10s, which a real-browser acceptance run
        // (`e2e/session-smoke.spec.ts`) showed left recently created Sessions
        // "missing" for several seconds after creation.
        let mut scan = tokio::time::interval(Duration::from_millis(500));
        let mut known_sessions = HashMap::<protocol::SessionId, ProjectedSession>::new();
        loop {
            tokio::select! {
                _ = shutdown.changed() => break,
                _ = scan.tick() => {
                    match sessions.list() {
                        Ok(current) => {
                            for session in current {
                                let projected = ProjectedSession::from_session(&session);
                                if known_sessions.get(&session.id) != Some(&projected) {
                                    known_sessions.insert(session.id, projected.clone());
                                    record(
                                        &store,
                                        &machine_id,
                                        &epoch,
                                        &control,
                                        |cursor| ProjectionChange::SessionUpsert { cursor, session: projected },
                                        session.updated_at,
                                    );
                                }
                            }
                        }
                        Err(error) => tracing::warn!(%error, "projection Session scan failed"),
                    }
                }
                received = events.recv() => match received {
                    Ok(event) => {
                        let at = event.at;
                        let projected = ProjectedEventSummary::from_event(&event);
                        record(
                            &store,
                            &machine_id,
                            &epoch,
                            &control,
                            |cursor| ProjectionChange::EventAppend { cursor, event: projected },
                            at,
                        );
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "projection Event receiver lagged; Session scan continues");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                },
                received = observations.recv() => match received {
                    Ok(DaemonMessage::MachineMetrics { metrics }) => {
                        let at = metrics.at;
                        let projected = project_machine_metrics(&metrics);
                        record(
                            &store,
                            &machine_id,
                            &epoch,
                            &control,
                            |cursor| ProjectionChange::MetricsReplace { cursor, metrics: projected },
                            at,
                        );
                    }
                    Ok(DaemonMessage::SessionMetrics { metrics }) => {
                        let at = metrics.at;
                        let projected = project_session_metrics(&metrics);
                        record(
                            &store,
                            &machine_id,
                            &epoch,
                            &control,
                            |cursor| ProjectionChange::MetricsReplace { cursor, metrics: projected },
                            at,
                        );
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(skipped, "projection Metrics receiver lagged; latest sample will replace it");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });
}

fn record(
    store: &Store,
    machine_id: &MachineId,
    epoch: &str,
    control: &Option<ControlService>,
    make: impl FnOnce(u64) -> ProjectionChange,
    at: Timestamp,
) {
    let report = match store.projection_sync_report(machine_id, epoch) {
        Ok(report) => report,
        Err(error) => {
            tracing::warn!(%error, "projection cursor could not be read");
            return;
        }
    };
    let cursor = report.current_cursor.saturating_add(1);
    let change = make(cursor);
    if let Err(error) = store.record_projection_change(
        machine_id,
        epoch,
        &change,
        at,
        ProjectionOutboxLimits::default(),
    ) {
        tracing::warn!(%error, "projection change could not be queued");
        return;
    }
    if let Some(control) = control {
        let batch = ProjectionBatch::Delta {
            schema_version: protocol::PROJECTION_SCHEMA_VERSION,
            machine_id: machine_id.clone(),
            source_epoch: epoch.into(),
            base_cursor: cursor.saturating_sub(1),
            through_cursor: cursor,
            sent_at: at,
            changes: vec![change],
        };
        if let Err(error) = control.accept_local_projection(batch) {
            tracing::warn!(message = %error.failure.message, "local control projection update failed");
        } else if let Err(error) = store.acknowledge_projection_cursor(machine_id, epoch, cursor) {
            tracing::warn!(%error, "local projection acknowledgement was not persisted");
        }
    }
}

fn project_machine_metrics(metrics: &MachineMetrics) -> ProjectedMetrics {
    let gpu_percent = metrics
        .gpus
        .iter()
        .map(|gpu| gpu.util_percent)
        .reduce(f32::max);
    let vram_used_bytes = (!metrics.gpus.is_empty()).then(|| {
        metrics.gpus.iter().fold(0_u64, |total, gpu| {
            total.saturating_add(gpu.vram_used_bytes)
        })
    });
    let vram_total_bytes = (!metrics.gpus.is_empty()).then(|| {
        metrics.gpus.iter().fold(0_u64, |total, gpu| {
            total.saturating_add(gpu.vram_total_bytes)
        })
    });
    ProjectedMetrics {
        scope: ProjectedMetricScope::Machine,
        session_id: None,
        at: metrics.at,
        cpu_percent: metrics.cpu_percent,
        ram_used_bytes: metrics.ram_used_bytes,
        ram_total_bytes: Some(metrics.ram_total_bytes),
        gpu_percent,
        vram_used_bytes,
        vram_total_bytes,
    }
}

fn project_session_metrics(metrics: &SessionMetrics) -> ProjectedMetrics {
    ProjectedMetrics {
        scope: ProjectedMetricScope::Session,
        session_id: Some(metrics.session_id),
        at: metrics.at,
        cpu_percent: metrics.totals.cpu_percent,
        ram_used_bytes: metrics.totals.rss_bytes,
        ram_total_bytes: None,
        gpu_percent: None,
        vram_used_bytes: Some(metrics.totals.gpu_mem_bytes),
        vram_total_bytes: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{GpuMetrics, ProcessTotals};

    #[test]
    fn metrics_projection_is_numeric_and_coalescible() {
        let machine = project_machine_metrics(&MachineMetrics {
            at: Timestamp(1),
            cpu_percent: 20.0,
            ram_used_bytes: 4,
            ram_total_bytes: 8,
            gpus: vec![GpuMetrics {
                index: 0,
                util_percent: 96.0,
                vram_used_bytes: 3,
                vram_total_bytes: 4,
                temp_c: Some(60.0),
            }],
        });
        assert_eq!(machine.gpu_percent, Some(96.0));
        assert_eq!(machine.vram_used_bytes, Some(3));

        let session = project_session_metrics(&SessionMetrics {
            session_id: protocol::SessionId(7),
            at: Timestamp(2),
            totals: ProcessTotals {
                cpu_percent: 40.0,
                rss_bytes: 10,
                gpu_mem_bytes: 11,
            },
        });
        assert_eq!(session.ram_total_bytes, None);
        assert_eq!(session.vram_used_bytes, Some(11));
    }
}
