//! The keeper behind each keeper-held Session (T5, migration 016).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{CaptureLevel, EventKind, MachineId, Session, SessionId, SessionState, Timestamp};
use store::{Store, StoredKeeper};

fn live(id: u64) -> Session {
    let mut session = Session::new(
        SessionId(id),
        MachineId("m".into()),
        "/bin/sh",
        "/work",
        Timestamp(100),
    );
    session.capture = CaptureLevel::Full;
    session
        .transition(SessionState::Idle, Timestamp(101))
        .unwrap();
    session
}

fn keeper(id: u64) -> StoredKeeper {
    StoredKeeper {
        session_id: id,
        socket_path: format!("/run/user/1000/sentinal-keepers-ab/{id}.sock"),
        secret_hex: "ab".repeat(32),
        osc_nonce: "cd".repeat(16),
        keeper_pid: 51_234,
        keeper_started_at: 1_792_771_200,
        shell_pid: 51_240,
        shell_started_at: 1_792_771_200,
        protocol: 1,
        scope_unit: Some(format!("sentinal-keeper-ab-{id}")),
        created_at: 1_792_771_200_123,
    }
}

#[test]
fn a_keeper_row_round_trips_and_only_live_sessions_list_theirs() {
    let store = Store::open_in_memory().unwrap();
    for id in [1, 2] {
        store.insert_session(&live(id)).unwrap();
        store.put_keeper(&keeper(id)).unwrap();
    }
    assert_eq!(store.keeper(SessionId(1)).unwrap(), Some(keeper(1)));
    assert_eq!(store.keeper(SessionId(9)).unwrap(), None);

    let mut ended = live(2);
    ended
        .transition(SessionState::Completed, Timestamp(200))
        .unwrap();
    store.update_session(&ended).unwrap();
    let listed: Vec<u64> = store
        .live_keepers()
        .unwrap()
        .iter()
        .map(|k| k.session_id)
        .collect();
    assert_eq!(listed, [1]);

    store.delete_keeper(SessionId(1)).unwrap();
    assert_eq!(store.keeper(SessionId(1)).unwrap(), None);
}

#[test]
fn after_a_restart_only_sessions_without_a_keeper_are_interrupted_at_once() {
    let store = Store::open_in_memory().unwrap();
    store.insert_session(&live(1)).unwrap();
    store.insert_session(&live(2)).unwrap();
    store.put_keeper(&keeper(2)).unwrap();

    let events = store.interrupt_orphaned_sessions(Timestamp(300)).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id, SessionId(1));
    assert_eq!(
        store.session(SessionId(2)).unwrap().unwrap().state,
        SessionState::Idle
    );
}

#[test]
fn a_keeper_that_cannot_be_reclaimed_interrupts_its_session_with_the_reason() {
    let store = Store::open_in_memory().unwrap();
    store.insert_session(&live(3)).unwrap();
    store.put_keeper(&keeper(3)).unwrap();
    let event = store
        .interrupt_session(
            SessionId(3),
            Timestamp(400),
            "Stopped: could not find this terminal.",
        )
        .unwrap()
        .unwrap();
    assert!(
        matches!(event.kind, EventKind::WarningDetected { ref message } if message.contains("could not find"))
    );
    assert_eq!(
        store.session(SessionId(3)).unwrap().unwrap().state,
        SessionState::Interrupted
    );
    assert_eq!(
        store.keeper(SessionId(3)).unwrap(),
        None,
        "its secret is not kept"
    );
    // Already ended: nothing more.
    assert_eq!(
        store
            .interrupt_session(SessionId(3), Timestamp(401), "again")
            .unwrap(),
        None
    );
}
