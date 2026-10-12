//! Durable projection outbox and control read-model tests.

#![allow(clippy::unwrap_used)]

use protocol::{
    CaptureLevel, MachineId, ProjectedMetricScope, ProjectedMetrics, ProjectedSession,
    ProjectionBatch, ProjectionChange, Session, SessionId, Timestamp, PROJECTION_SCHEMA_VERSION,
};
use store::{ProjectionOutboxLimits, Store};

const MACHINE: &str = "01M40Q6XWORKER";
const EPOCH: &str = "pe_01M4D1";

fn projected_session(id: u64, at: i64) -> ProjectedSession {
    let mut session = Session::new(
        SessionId(id),
        MachineId(MACHINE.into()),
        "/bin/zsh",
        "/private/path",
        Timestamp(at),
    );
    session.title = Some(format!("Session {id}"));
    session.capture = CaptureLevel::Full;
    ProjectedSession::from_session(&session)
}

fn upsert(cursor: u64, id: u64) -> ProjectionChange {
    ProjectionChange::SessionUpsert {
        cursor,
        session: projected_session(id, cursor as i64),
    }
}

fn metrics(cursor: u64, session_id: u64, cpu_percent: f32) -> ProjectionChange {
    ProjectionChange::MetricsReplace {
        cursor,
        metrics: ProjectedMetrics {
            scope: ProjectedMetricScope::Session,
            session_id: Some(SessionId(session_id)),
            at: Timestamp(cursor as i64),
            cpu_percent,
            ram_used_bytes: cursor,
            ram_total_bytes: None,
            gpu_percent: None,
            vram_used_bytes: None,
            vram_total_bytes: None,
        },
    }
}

#[test]
fn migration_seven_adds_projection_tables_without_resetting_existing_data() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.schema_version().unwrap(), 8);
    for table in [
        "projection_worker_state",
        "projection_outbox",
        "control_projection_state",
        "control_projection_sessions",
        "control_projection_events",
        "control_projection_metrics",
        "control_projection_changes",
        "worker_projection_records",
        "control_projection_snapshot_chunks",
    ] {
        assert!(store.has_table(table).unwrap(), "missing {table}");
    }
}

#[test]
fn worker_current_projection_survives_ack_for_full_resync() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let limits = ProjectionOutboxLimits::default();
    store
        .record_projection_change(&machine, EPOCH, &upsert(1, 10), Timestamp(1), limits)
        .unwrap();
    store
        .record_projection_change(&machine, EPOCH, &metrics(2, 10, 50.0), Timestamp(2), limits)
        .unwrap();
    store
        .acknowledge_projection_cursor(&machine, EPOCH, 2)
        .unwrap();

    assert!(store
        .projection_changes_after(&machine, EPOCH, 0, 10)
        .unwrap()
        .is_empty());
    let snapshot = store.worker_projection_snapshot(&machine, EPOCH).unwrap();
    assert_eq!(snapshot.len(), 2);
    assert_eq!(snapshot[0].cursor(), 1);
    assert_eq!(snapshot[1].cursor(), 2);
}

#[test]
fn worker_outbox_is_ordered_restart_safe_and_acknowledged_by_cursor() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let limits = ProjectionOutboxLimits::default();
    store
        .record_projection_change(&machine, EPOCH, &upsert(1, 10), Timestamp(1), limits)
        .unwrap();
    store
        .record_projection_change(&machine, EPOCH, &upsert(2, 11), Timestamp(2), limits)
        .unwrap();

    let report = store.projection_sync_report(&machine, EPOCH).unwrap();
    assert_eq!(report.current_cursor, 2);
    assert_eq!(report.last_acked_cursor, 0);
    assert_eq!(
        store
            .projection_changes_after(&machine, EPOCH, 0, 128)
            .unwrap(),
        vec![upsert(1, 10), upsert(2, 11)]
    );

    assert_eq!(
        store
            .acknowledge_projection_cursor(&machine, EPOCH, 1)
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .projection_changes_after(&machine, EPOCH, 1, 128)
            .unwrap(),
        vec![upsert(2, 11)]
    );
    assert_eq!(
        store
            .projection_sync_report(&machine, EPOCH)
            .unwrap()
            .last_acked_cursor,
        1
    );
}

#[test]
fn worker_outbox_rejects_gaps_and_stays_bounded() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let one_row = ProjectionOutboxLimits {
        maximum_rows: 1,
        maximum_bytes: 64 * 1024,
    };
    assert!(store
        .record_projection_change(&machine, EPOCH, &upsert(2, 10), Timestamp(1), one_row)
        .is_err());
    store
        .record_projection_change(&machine, EPOCH, &upsert(1, 10), Timestamp(1), one_row)
        .unwrap();
    assert!(store
        .record_projection_change(&machine, EPOCH, &upsert(2, 11), Timestamp(2), one_row)
        .is_err());
}

#[test]
fn worker_outbox_coalesces_replaceable_metrics_without_cursor_gaps() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let limits = ProjectionOutboxLimits::default();
    store
        .record_projection_change(&machine, EPOCH, &metrics(1, 10, 10.0), Timestamp(1), limits)
        .unwrap();
    store
        .record_projection_change(&machine, EPOCH, &metrics(2, 10, 90.0), Timestamp(2), limits)
        .unwrap();

    let report = store.projection_sync_report(&machine, EPOCH).unwrap();
    assert_eq!(report.current_cursor, 1);
    let mut expected = metrics(2, 10, 90.0);
    if let ProjectionChange::MetricsReplace { cursor, .. } = &mut expected {
        *cursor = 1;
    }
    assert_eq!(
        store
            .projection_changes_after(&machine, EPOCH, 0, 128)
            .unwrap(),
        vec![expected]
    );
}

#[test]
fn worker_outbox_keeps_terminal_session_transition() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let limits = ProjectionOutboxLimits::default();
    store
        .record_projection_change(&machine, EPOCH, &upsert(1, 10), Timestamp(1), limits)
        .unwrap();
    let mut terminal = projected_session(10, 2);
    terminal.state = protocol::SessionState::Completed;
    store
        .record_projection_change(
            &machine,
            EPOCH,
            &ProjectionChange::SessionUpsert {
                cursor: 2,
                session: terminal,
            },
            Timestamp(2),
            limits,
        )
        .unwrap();

    assert_eq!(
        store
            .projection_changes_after(&machine, EPOCH, 0, 128)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn control_delta_is_atomic_idempotent_and_machine_bound() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let batch = ProjectionBatch::Delta {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id: machine.clone(),
        source_epoch: EPOCH.into(),
        base_cursor: 0,
        through_cursor: 2,
        sent_at: Timestamp(2),
        changes: vec![upsert(1, 10), upsert(2, 11)],
    };
    let first = store
        .apply_projection_batch(&machine, &batch, Timestamp(3))
        .unwrap();
    assert_eq!(first.committed_cursor, 2);
    assert_eq!(store.projected_sessions(&machine, 10).unwrap().len(), 2);

    let duplicate = store
        .apply_projection_batch(&machine, &batch, Timestamp(4))
        .unwrap();
    assert_eq!(
        duplicate.status,
        protocol::ProjectionAcknowledgementStatus::Duplicate
    );
    assert_eq!(store.projected_sessions(&machine, 10).unwrap().len(), 2);

    let wrong = MachineId("01M40OTHER".into());
    assert!(store
        .apply_projection_batch(&wrong, &batch, Timestamp(5))
        .is_err());
    assert!(store.projected_sessions(&wrong, 10).unwrap().is_empty());
}

#[test]
fn cursor_mismatch_requests_resync_without_partial_rows() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let batch = ProjectionBatch::Delta {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id: machine.clone(),
        source_epoch: EPOCH.into(),
        base_cursor: 9,
        through_cursor: 10,
        sent_at: Timestamp(2),
        changes: vec![upsert(10, 99)],
    };
    let acknowledgement = store
        .apply_projection_batch(&machine, &batch, Timestamp(3))
        .unwrap();
    assert_eq!(
        acknowledgement.status,
        protocol::ProjectionAcknowledgementStatus::ResyncRequired
    );
    assert_eq!(
        acknowledgement.reason,
        Some(protocol::ProjectionResyncReason::CursorMismatch)
    );
    assert!(store.projected_sessions(&machine, 10).unwrap().is_empty());
}

#[test]
fn snapshot_chunks_replace_projection_only_after_the_final_chunk() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    store
        .apply_projection_batch(
            &machine,
            &ProjectionBatch::Delta {
                schema_version: PROJECTION_SCHEMA_VERSION,
                machine_id: machine.clone(),
                source_epoch: "old_epoch".into(),
                base_cursor: 0,
                through_cursor: 1,
                sent_at: Timestamp(1),
                changes: vec![upsert(1, 99)],
            },
            Timestamp(1),
        )
        .unwrap();
    let first = ProjectionBatch::Snapshot {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id: machine.clone(),
        source_epoch: EPOCH.into(),
        snapshot_id: "snapshot_1".into(),
        snapshot_cursor: 2,
        chunk_index: 0,
        final_chunk: false,
        sent_at: Timestamp(2),
        records: vec![upsert(1, 10)],
    };
    let accepted = store
        .apply_projection_batch(&machine, &first, Timestamp(2))
        .unwrap();
    assert_eq!(accepted.committed_cursor, 1);
    assert_eq!(
        store.projected_sessions(&machine, 10).unwrap()[0].id,
        SessionId(99)
    );

    let final_chunk = ProjectionBatch::Snapshot {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id: machine.clone(),
        source_epoch: EPOCH.into(),
        snapshot_id: "snapshot_1".into(),
        snapshot_cursor: 2,
        chunk_index: 1,
        final_chunk: true,
        sent_at: Timestamp(3),
        records: vec![upsert(2, 11)],
    };
    let accepted = store
        .apply_projection_batch(&machine, &final_chunk, Timestamp(3))
        .unwrap();
    assert_eq!(accepted.committed_cursor, 2);
    let sessions = store.projected_sessions(&machine, 10).unwrap();
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().all(|session| session.id != SessionId(99)));
}

#[test]
fn snapshot_rejects_an_out_of_order_chunk_without_replacing_rows() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let batch = ProjectionBatch::Snapshot {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id: machine.clone(),
        source_epoch: EPOCH.into(),
        snapshot_id: "snapshot_1".into(),
        snapshot_cursor: 2,
        chunk_index: 1,
        final_chunk: true,
        sent_at: Timestamp(2),
        records: vec![upsert(2, 11)],
    };
    let result = store
        .apply_projection_batch(&machine, &batch, Timestamp(3))
        .unwrap();
    assert_eq!(
        result.status,
        protocol::ProjectionAcknowledgementStatus::ResyncRequired
    );
    assert!(store.projected_sessions(&machine, 10).unwrap().is_empty());
}

#[test]
fn projection_rows_never_contain_private_session_fields() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let batch = ProjectionBatch::Delta {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id: machine.clone(),
        source_epoch: EPOCH.into(),
        base_cursor: 0,
        through_cursor: 1,
        sent_at: Timestamp(1),
        changes: vec![upsert(1, 10)],
    };
    store
        .apply_projection_batch(&machine, &batch, Timestamp(2))
        .unwrap();
    let encoded = serde_json::to_string(&store.projected_sessions(&machine, 10).unwrap()).unwrap();
    for forbidden in ["/private/path", "/bin/zsh", "current_command", "evidence"] {
        assert!(
            !encoded.contains(forbidden),
            "projection leaked {forbidden}"
        );
    }
}

#[test]
fn control_change_replay_is_revision_ordered_and_bounded() {
    let store = Store::open_in_memory().unwrap();
    let machine = MachineId(MACHINE.into());
    let batch = ProjectionBatch::Delta {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id: machine.clone(),
        source_epoch: EPOCH.into(),
        base_cursor: 0,
        through_cursor: 2,
        sent_at: Timestamp(2),
        changes: vec![upsert(1, 10), upsert(2, 11)],
    };
    store
        .apply_projection_batch(&machine, &batch, Timestamp(3))
        .unwrap();

    let first = store.control_projection_changes_after(0, 1).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].revision, 1);
    let second = store.control_projection_changes_after(1, 10).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].revision, 2);
    assert_eq!(second[0].machine_id, machine);
}

#[test]
fn purging_one_machine_projection_preserves_another() {
    let store = Store::open_in_memory().unwrap();
    let first = MachineId(MACHINE.into());
    let second = MachineId("01M40Q6XSECOND".into());
    for machine in [&first, &second] {
        store
            .apply_projection_batch(
                machine,
                &ProjectionBatch::Delta {
                    schema_version: PROJECTION_SCHEMA_VERSION,
                    machine_id: machine.clone(),
                    source_epoch: EPOCH.into(),
                    base_cursor: 0,
                    through_cursor: 1,
                    sent_at: Timestamp(1),
                    changes: vec![upsert(1, 10)],
                },
                Timestamp(2),
            )
            .unwrap();
    }

    store.purge_control_projection(&first).unwrap();
    assert!(store.projected_sessions(&first, 10).unwrap().is_empty());
    assert!(store.control_projection_state(&first).unwrap().is_none());
    assert_eq!(store.projected_sessions(&second, 10).unwrap().len(), 1);
}
