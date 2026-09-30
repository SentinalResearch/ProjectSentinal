//! SQLite migration, history, retention, and recovery behaviour tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{
    CaptureLevel, Command, CommandId, Event, EventId, EventKind, MachineId, Session, SessionId,
    SessionState, Timestamp,
};
use std::sync::atomic::{AtomicU64, Ordering};
use store::{RawMetricSample, Store};

static NEXT_DATABASE: AtomicU64 = AtomicU64::new(1);

fn session(id: u64, state: SessionState) -> Session {
    let mut session = Session::new(
        SessionId(id),
        MachineId("test-machine".into()),
        "/bin/sh",
        "/work",
        Timestamp(100),
    );
    session
        .transition(SessionState::Idle, Timestamp(101))
        .unwrap();
    if state == SessionState::Running {
        session
            .transition(SessionState::Running, Timestamp(102))
            .unwrap();
    }
    session
}

#[test]
fn persisted_sessions_are_listed_in_id_order_and_empty_is_valid() {
    let store = Store::open_in_memory().unwrap();
    assert!(store.sessions().unwrap().is_empty());
    store
        .insert_session(&session(9, SessionState::Idle))
        .unwrap();
    store
        .insert_session(&session(3, SessionState::Idle))
        .unwrap();

    let listed = store.sessions().unwrap();

    assert_eq!(
        listed.iter().map(|value| value.id.0).collect::<Vec<_>>(),
        vec![3, 9]
    );
}

fn command(id: u64, session_id: u64, text: &str, at: i64) -> Command {
    Command::start(
        CommandId(id),
        SessionId(session_id),
        MachineId("test-machine".into()),
        text,
        "/work",
        Timestamp(at),
    )
}

#[test]
fn migrations_from_empty_database_enable_wal_and_create_every_table() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.schema_version().unwrap(), 2);
    assert_eq!(store.journal_mode().unwrap(), "memory");
    for table in [
        "machines",
        "sessions",
        "commands",
        "commands_fts",
        "events",
        "metrics_raw",
        "metrics_minute",
    ] {
        assert!(store.has_table(table).unwrap(), "missing table {table}");
    }
}

#[test]
fn file_database_uses_wal_mode() {
    let path = std::env::temp_dir().join(format!(
        "terminal-agent-store-{}-{}.sqlite3",
        std::process::id(),
        NEXT_DATABASE.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Store::open(&path).unwrap();
    assert_eq!(store.journal_mode().unwrap(), "wal");
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn updating_an_unknown_session_returns_a_named_error() {
    let store = Store::open_in_memory().unwrap();
    let error = store
        .update_session(&session(404, SessionState::Idle))
        .unwrap_err();
    assert!(error.to_string().contains("session 404"));
}

#[test]
fn history_is_newest_first_and_can_be_limited_to_a_session() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_session(&session(1, SessionState::Idle))
        .unwrap();
    store
        .insert_session(&session(2, SessionState::Idle))
        .unwrap();
    store.insert_command(&command(1, 1, "old", 100)).unwrap();
    store.insert_command(&command(2, 2, "other", 200)).unwrap();
    store.insert_command(&command(3, 1, "new", 300)).unwrap();

    let all = store.history(None, 2).unwrap();
    assert_eq!(
        all.iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["new", "other"]
    );
    let first = store.history(Some(SessionId(1)), 10).unwrap();
    assert_eq!(
        first
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>(),
        ["new", "old"]
    );
}

#[test]
fn fts_finds_train_inside_a_command() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_session(&session(1, SessionState::Idle))
        .unwrap();
    store
        .insert_command(&command(1, 1, "python train.py --epochs 3", 100))
        .unwrap();
    store
        .insert_command(&command(2, 1, "cargo build", 200))
        .unwrap();

    let found = store.search_commands("train", 10).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].text, "python train.py --epochs 3");
}

#[test]
fn finish_command_updates_exit_and_finish_time() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_session(&session(1, SessionState::Idle))
        .unwrap();
    let mut value = command(1, 1, "false", 100);
    store.insert_command(&value).unwrap();
    value.finish(1, Timestamp(142)).unwrap();

    store.finish_command(&value).unwrap();

    assert_eq!(store.history(None, 10).unwrap(), vec![value]);
}

#[test]
fn events_since_returns_only_later_ids() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_session(&session(1, SessionState::Idle))
        .unwrap();
    for id in 1..=3 {
        store
            .insert_event(&Event {
                id: EventId(id),
                session_id: SessionId(1),
                command_id: None,
                at: Timestamp(i64::try_from(id).unwrap()),
                kind: EventKind::WarningDetected {
                    message: format!("event {id}"),
                },
            })
            .unwrap();
    }
    let events = store.events_since(EventId(1)).unwrap();
    assert_eq!(
        events.iter().map(|event| event.id.0).collect::<Vec<_>>(),
        [2, 3]
    );
}

#[test]
fn downsampling_averages_expired_raw_metrics_and_keeps_recent_samples() {
    let store = Store::open_in_memory().unwrap();
    for (at, cpu, ram) in [
        (60_000, 20.0, 100),
        (70_000, 40.0, 300),
        (3_700_000, 90.0, 900),
    ] {
        store
            .insert_raw_metric(&RawMetricSample {
                session_id: Some(SessionId(1)),
                at: Timestamp(at),
                cpu_percent: cpu,
                ram_used_bytes: ram,
                ram_total_bytes: 1_000,
                gpu_percent: Some(cpu),
                vram_used_bytes: Some(ram),
                vram_total_bytes: Some(1_000),
            })
            .unwrap();
    }

    assert_eq!(store.downsample_metrics(Timestamp(3_720_000)).unwrap(), 2);
    let minute = store.minute_metrics().unwrap();
    assert_eq!(minute.len(), 1);
    assert_eq!(minute[0].cpu_percent, 30.0);
    assert_eq!(minute[0].ram_used_bytes, 200);
    assert_eq!(store.raw_metric_count().unwrap(), 1);
}

#[test]
fn downsampling_empty_metrics_is_a_no_op() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.downsample_metrics(Timestamp(3_700_000)).unwrap(), 0);
    assert!(store.minute_metrics().unwrap().is_empty());
}

#[test]
fn downsampling_waits_until_a_minute_is_complete() {
    let store = Store::open_in_memory().unwrap();
    for (at, cpu) in [(60_000, 20.0), (119_999, 40.0)] {
        store
            .insert_raw_metric(&RawMetricSample {
                session_id: None,
                at: Timestamp(at),
                cpu_percent: cpu,
                ram_used_bytes: 100,
                ram_total_bytes: 1_000,
                gpu_percent: None,
                vram_used_bytes: None,
                vram_total_bytes: None,
            })
            .unwrap();
    }

    assert_eq!(store.downsample_metrics(Timestamp(3_719_999)).unwrap(), 0);
    assert!(store.minute_metrics().unwrap().is_empty());
    assert_eq!(store.downsample_metrics(Timestamp(3_720_000)).unwrap(), 2);
    assert_eq!(store.minute_metrics().unwrap()[0].cpu_percent, 30.0);
}

#[test]
fn restart_marks_live_sessions_interrupted_and_records_an_event() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_session(&session(1, SessionState::Running))
        .unwrap();
    store
        .insert_session(&session(2, SessionState::Idle))
        .unwrap();
    let mut completed = session(3, SessionState::Idle);
    completed
        .transition(SessionState::Completed, Timestamp(200))
        .unwrap();
    store.insert_session(&completed).unwrap();

    let recovered = store.interrupt_orphaned_sessions(Timestamp(500)).unwrap();

    assert_eq!(recovered.len(), 2);
    assert_eq!(
        store.session(SessionId(1)).unwrap().unwrap().state,
        SessionState::Interrupted
    );
    assert_eq!(
        store.session(SessionId(2)).unwrap().unwrap().state,
        SessionState::Interrupted
    );
    assert_eq!(
        store.session(SessionId(3)).unwrap().unwrap().state,
        SessionState::Completed
    );
    assert_eq!(store.events_since(EventId(0)).unwrap().len(), 2);
}

#[test]
fn pre_1b_session_json_loads_with_safe_defaults_and_updates_without_migration() {
    let path = std::env::temp_dir().join(format!(
        "terminal-agent-store-legacy-{}-{}.sqlite3",
        std::process::id(),
        NEXT_DATABASE.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Store::open(&path).unwrap();
    store
        .insert_session(&session(1, SessionState::Idle))
        .unwrap();
    drop(store);

    let legacy = r#"{"id":1,"machine_id":"test-machine","project_id":null,"title":null,"shell":"/bin/sh","cwd":"/work","state":"IDLE","current_command":null,"created_at":100,"updated_at":101}"#;
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute("UPDATE sessions SET json=?1 WHERE id=1", [legacy])
        .unwrap();
    drop(connection);

    let store = Store::open(&path).unwrap();
    let loaded = store.session(SessionId(1)).unwrap().unwrap();
    assert_eq!(loaded.origin, protocol::SessionOrigin::Ui);
    assert_eq!(loaded.capture, protocol::CaptureLevel::Full);
    assert_eq!(loaded.pid, None);
    store.update_session(&loaded).unwrap();
    assert_eq!(store.schema_version().unwrap(), 2);
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn invalid_persisted_origin_capture_is_rejected_after_deserialization() {
    let path = std::env::temp_dir().join(format!(
        "terminal-agent-store-invalid-capture-{}-{}.sqlite3",
        std::process::id(),
        NEXT_DATABASE.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Store::open(&path).unwrap();
    store
        .insert_session(&session(1, SessionState::Idle))
        .unwrap();
    drop(store);

    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE sessions SET json=json_set(json, '$.origin', 'ADOPTED', '$.capture', 'FULL') WHERE id=1",
            [],
        )
        .unwrap();
    drop(connection);

    let store = Store::open(&path).unwrap();
    let error = store.session(SessionId(1)).unwrap_err();
    assert!(error.to_string().contains("ADOPTED/FULL"));
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn restart_interrupts_only_daemon_owned_sessions_and_lists_the_rest_as_live() {
    let store = Store::open_in_memory().unwrap();
    let full = session(1, SessionState::Idle);
    let mut hooked = session(2, SessionState::Idle);
    hooked.origin = protocol::SessionOrigin::ShellHook;
    hooked.capture = protocol::CaptureLevel::Commands;
    hooked.pid = Some(4242);
    hooked.process_started_at = Some(Timestamp(50));
    store.insert_session(&full).unwrap();
    store.insert_session(&hooked).unwrap();

    let events = store.interrupt_orphaned_sessions(Timestamp(200)).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, SessionId(1));
    assert_eq!(
        store.session(SessionId(1)).unwrap().unwrap().state,
        SessionState::Interrupted
    );
    assert_eq!(store.live_sessions().unwrap(), vec![hooked]);
}

fn finished_session(id: u64) -> Session {
    let mut value = session(id, SessionState::Idle);
    value
        .transition(SessionState::Completed, Timestamp(200))
        .unwrap();
    value.exit_code = Some(0);
    value
}

fn continuation(id: u64, predecessor: u64) -> Session {
    let mut value = session(id, SessionState::Idle);
    value.continued_from = Some(SessionId(predecessor));
    value
}

#[test]
fn lineage_is_queryable_from_every_generation_and_has_one_successor() {
    let store = Store::open_in_memory().unwrap();
    store.insert_session(&finished_session(1)).unwrap();
    let mut second = continuation(2, 1);
    store.insert_continuation(SessionId(1), &second).unwrap();
    second
        .transition(SessionState::Completed, Timestamp(300))
        .unwrap();
    second.exit_code = Some(0);
    store.update_session(&second).unwrap();
    let third = continuation(3, 2);
    store.insert_continuation(SessionId(2), &third).unwrap();

    assert_eq!(store.predecessor(SessionId(1)).unwrap(), None);
    assert_eq!(
        store.predecessor(SessionId(2)).unwrap().unwrap().id,
        SessionId(1)
    );
    assert_eq!(
        store.direct_successor(SessionId(1)).unwrap().unwrap().id,
        SessionId(2)
    );
    assert_eq!(
        store
            .ordered_lineage(SessionId(2))
            .unwrap()
            .iter()
            .map(|value| value.id.0)
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(
        store.latest_generation(SessionId(1)).unwrap().id,
        SessionId(3)
    );

    let error = store
        .insert_continuation(SessionId(1), &continuation(4, 1))
        .unwrap_err();
    assert!(error.to_string().contains("successor Session #2"));
}

#[test]
fn continuation_rejects_missing_self_cross_machine_live_and_reduced_sources() {
    let store = Store::open_in_memory().unwrap();
    let missing = store
        .insert_continuation(SessionId(99), &continuation(2, 99))
        .unwrap_err();
    assert!(missing.to_string().contains("session 99"));

    let mut self_reference = continuation(3, 3);
    let self_error = store
        .insert_continuation(SessionId(3), &self_reference)
        .unwrap_err();
    assert!(self_error.to_string().contains("itself"));

    store.insert_session(&finished_session(1)).unwrap();
    self_reference.continued_from = Some(SessionId(1));
    self_reference.machine_id = MachineId("another-machine".into());
    let cross_machine = store
        .insert_continuation(SessionId(1), &self_reference)
        .unwrap_err();
    assert!(cross_machine.to_string().contains("another-machine"));

    store
        .insert_session(&session(10, SessionState::Idle))
        .unwrap();
    let live = store
        .insert_continuation(SessionId(10), &continuation(11, 10))
        .unwrap_err();
    assert!(live.to_string().contains("Idle"));

    let mut reduced = finished_session(20);
    reduced.origin = protocol::SessionOrigin::ShellHook;
    reduced.capture = CaptureLevel::Commands;
    reduced.exit_code = None;
    store.insert_session(&reduced).unwrap();
    let reduced_error = store
        .insert_continuation(SessionId(20), &continuation(21, 20))
        .unwrap_err();
    assert!(reduced_error.to_string().contains("COMMANDS"));
}

#[test]
fn session_updates_cannot_change_the_authoritative_lineage_column() {
    let store = Store::open_in_memory().unwrap();
    store.insert_session(&finished_session(1)).unwrap();
    let mut child = continuation(2, 1);
    store.insert_continuation(SessionId(1), &child).unwrap();

    child.continued_from = None;
    let error = store.update_session(&child).unwrap_err();
    assert!(error.to_string().contains("lineage"));
    assert_eq!(
        store.session(SessionId(2)).unwrap().unwrap().continued_from,
        Some(SessionId(1))
    );
}

#[test]
fn version_one_database_migrates_without_resetting_legacy_sessions() {
    let path = std::env::temp_dir().join(format!(
        "terminal-agent-store-lineage-migration-{}-{}.sqlite3",
        std::process::id(),
        NEXT_DATABASE.fetch_add(1, Ordering::Relaxed)
    ));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE schema_version (version INTEGER PRIMARY KEY NOT NULL);\
             INSERT INTO schema_version(version) VALUES (1);",
        )
        .unwrap();
    connection
        .execute_batch(include_str!("../migrations/001_initial.sql"))
        .unwrap();
    let legacy = r#"{"id":1,"machine_id":"test-machine","project_id":null,"title":null,"shell":"/bin/sh","cwd":"/work","state":"COMPLETED","current_command":null,"origin":"UI","capture":"FULL","pid":null,"process_started_at":null,"exit_code":0,"log":null,"created_at":100,"updated_at":200}"#;
    connection
        .execute(
            "INSERT INTO sessions(id,state,updated_at,json) VALUES (1,'COMPLETED',200,?1)",
            [legacy],
        )
        .unwrap();
    drop(connection);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.schema_version().unwrap(), 2);
    assert_eq!(
        store.session(SessionId(1)).unwrap().unwrap().continued_from,
        None
    );
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn malformed_persisted_cycles_and_missing_ancestors_do_not_loop() {
    let path = std::env::temp_dir().join(format!(
        "terminal-agent-store-malformed-lineage-{}-{}.sqlite3",
        std::process::id(),
        NEXT_DATABASE.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Store::open(&path).unwrap();
    store.insert_session(&finished_session(1)).unwrap();
    store.insert_session(&finished_session(2)).unwrap();
    drop(store);

    let mut first = finished_session(1);
    first.continued_from = Some(SessionId(2));
    let mut second = finished_session(2);
    second.continued_from = Some(SessionId(1));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE sessions SET continued_from=2, json=?1 WHERE id=1",
            [serde_json::to_string(&first).unwrap()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE sessions SET continued_from=1, json=?1 WHERE id=2",
            [serde_json::to_string(&second).unwrap()],
        )
        .unwrap();
    drop(connection);

    let store = Store::open(&path).unwrap();
    let cycle = store.ordered_lineage(SessionId(1)).unwrap_err();
    assert!(cycle.to_string().contains("cycle"));
    drop(store);

    second.continued_from = Some(SessionId(999));
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .pragma_update(None, "foreign_keys", "OFF")
        .unwrap();
    connection
        .execute(
            "UPDATE sessions SET continued_from=999, json=?1 WHERE id=2",
            [serde_json::to_string(&second).unwrap()],
        )
        .unwrap();
    drop(connection);
    let store = Store::open(&path).unwrap();
    let missing = store.ordered_lineage(SessionId(2)).unwrap_err();
    assert!(missing.to_string().contains("missing Session #999"));
    drop(store);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn mismatched_json_and_lineage_column_is_rejected_on_read() {
    let path = std::env::temp_dir().join(format!(
        "terminal-agent-store-lineage-drift-{}-{}.sqlite3",
        std::process::id(),
        NEXT_DATABASE.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Store::open(&path).unwrap();
    store.insert_session(&finished_session(1)).unwrap();
    store.insert_session(&finished_session(2)).unwrap();
    drop(store);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection
        .execute("UPDATE sessions SET continued_from=1 WHERE id=2", [])
        .unwrap();
    drop(connection);

    let store = Store::open(&path).unwrap();
    let error = store.session(SessionId(2)).unwrap_err();
    assert!(error.to_string().contains("lineage mismatch"));
    drop(store);
    std::fs::remove_file(path).unwrap();
}
