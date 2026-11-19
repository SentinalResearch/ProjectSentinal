//! T0.12: one stored row this build cannot read (written by a newer release,
//! before a rollback) is skipped and logged; the query returns the rest.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{Event, EventId, EventKind, MachineId, Session, SessionId, SessionState, Timestamp};
use store::Store;

fn session(id: u64) -> Session {
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
    session
}

fn database(name: &str) -> std::path::PathBuf {
    let dir =
        std::env::temp_dir().join(format!("sentinal-unreadable-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("sentinal.db")
}

#[test]
fn a_session_from_a_newer_release_is_skipped_not_fatal() {
    let path = database("sessions");
    let store = Store::open(&path).unwrap();
    store.insert_session(&session(1)).unwrap();
    store.insert_session(&session(2)).unwrap();
    // As a newer release would store it: a coding agent this build does not know.
    let raw = rusqlite::Connection::open(&path).unwrap();
    let json: String = raw
        .query_row("SELECT json FROM sessions WHERE id=2", [], |row| row.get(0))
        .unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value["coding_agent"] = serde_json::json!({ "agent": "FUTURE_AGENT" });
    raw.execute(
        "UPDATE sessions SET json=?1 WHERE id=2",
        [value.to_string()],
    )
    .unwrap();

    let ids: Vec<u64> = store.sessions().unwrap().iter().map(|s| s.id.0).collect();
    assert_eq!(ids, vec![1]);
    let live: Vec<u64> = store
        .live_sessions()
        .unwrap()
        .iter()
        .map(|s| s.id.0)
        .collect();
    assert_eq!(live, vec![1]);
    assert!(store.interrupt_orphaned_sessions(Timestamp(200)).is_ok());
}

#[test]
fn an_event_kind_from_a_newer_release_is_skipped_not_fatal() {
    let path = database("events");
    let store = Store::open(&path).unwrap();
    store.insert_session(&session(1)).unwrap();
    for id in 1..=3 {
        store
            .insert_event(&Event {
                id: EventId(id),
                session_id: SessionId(1),
                command_id: None,
                at: Timestamp(300 + id as i64),
                kind: EventKind::WarningDetected {
                    message: format!("warning {id}"),
                },
            })
            .unwrap();
    }
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "UPDATE events SET kind_json='{\"type\":\"FUTURE_EVENT\",\"x\":1}' WHERE id=2",
        [],
    )
    .unwrap();

    let ids: Vec<u64> = store
        .events(Some(SessionId(1)), None, 10)
        .unwrap()
        .iter()
        .map(|event| event.id.0)
        .collect();
    assert_eq!(ids, vec![1, 3]);
}
