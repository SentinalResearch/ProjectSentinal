//! Deleting a Session erases what is stored about it and never reuses its IDs.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{
    Command, CommandId, Event, EventId, EventKind, MachineId, Session, SessionId, SessionState,
    Timestamp,
};
use store::Store;

fn session(id: u64, state: SessionState) -> Session {
    let mut session = Session::new(
        SessionId(id),
        MachineId("m".into()),
        "/bin/sh",
        "/work",
        Timestamp(1),
    );
    session.state = state;
    session
}

fn seed(store: &Store, id: u64) {
    store
        .insert_session(&session(id, SessionState::Completed))
        .unwrap();
    store
        .insert_command(&Command::start(
            CommandId(id * 10),
            SessionId(id),
            MachineId("m".into()),
            format!("secret-command-{id}"),
            "/work",
            Timestamp(1),
        ))
        .unwrap();
    store
        .insert_event(&Event {
            id: EventId(id * 10),
            session_id: SessionId(id),
            command_id: None,
            at: Timestamp(1),
            kind: EventKind::ErrorDetected {
                message: format!("secret-error-{id}"),
            },
        })
        .unwrap();
}

#[test]
fn deleting_erases_rows_and_search_text_of_that_session_only() {
    let store = Store::open_in_memory().unwrap();
    seed(&store, 1);
    seed(&store, 2);

    store.delete_sessions(&[SessionId(1)]).unwrap();

    assert!(store.session(SessionId(1)).unwrap().is_none());
    assert!(store
        .events(Some(SessionId(1)), None, 10)
        .unwrap()
        .is_empty());
    assert_eq!(store.events(Some(SessionId(2)), None, 10).unwrap().len(), 1);
    let text = |query: &str| store.search_commands(query, 10).unwrap().len();
    assert_eq!(text("secret-command-1"), 0);
    assert_eq!(text("secret-command-2"), 1);
}

#[test]
fn ids_of_deleted_rows_are_never_reused() {
    let store = Store::open_in_memory().unwrap();
    seed(&store, 5);
    store.delete_sessions(&[SessionId(5)]).unwrap();
    assert_eq!(store.next_session_id().unwrap(), 6);
    assert_eq!(store.next_command_id().unwrap(), 51);
    assert_eq!(store.next_event_id().unwrap(), 51);
}

#[test]
fn deleting_an_unknown_session_changes_nothing() {
    let store = Store::open_in_memory().unwrap();
    seed(&store, 1);
    assert!(store
        .delete_sessions(&[SessionId(1), SessionId(9)])
        .is_err());
    assert!(store.session(SessionId(1)).unwrap().is_some());
}
