//! Durable projection outbox and control read-model tests.

#![allow(clippy::unwrap_used)]

use protocol::{
    CaptureLevel, MachineId, ProjectedSession, ProjectionBatch, ProjectionChange, Session,
    SessionId, Timestamp, PROJECTION_SCHEMA_VERSION,
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

#[test]
fn migration_seven_adds_projection_tables_without_resetting_existing_data() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.schema_version().unwrap(), 7);
    for table in [
        "projection_worker_state",
        "projection_outbox",
        "control_projection_state",
        "control_projection_sessions",
        "control_projection_events",
        "control_projection_metrics",
        "control_projection_changes",
    ] {
        assert!(store.has_table(table).unwrap(), "missing {table}");
    }
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
