//! Adopted-log cursor persistence.

#![allow(clippy::unwrap_used)]

use protocol::{MachineId, Session, SessionId, Timestamp};
use store::{Store, StoredLogCursor};

fn store_with_session(id: u64) -> Store {
    let store = Store::open_in_memory().unwrap();
    let session = Session::new(
        SessionId(id),
        MachineId("test-machine".into()),
        "/usr/bin/python3",
        "/work",
        Timestamp(100),
    );
    store.insert_session(&session).unwrap();
    store
}

fn cursor(offset: u64) -> StoredLogCursor {
    StoredLogCursor {
        path: "/work/train.log".into(),
        device: u64::MAX - 7,
        inode: 42,
        offset,
    }
}

#[test]
fn saved_cursor_round_trips_including_high_device_numbers() {
    let store = store_with_session(1);
    store.save_log_cursor(SessionId(1), &cursor(10)).unwrap();
    assert_eq!(store.log_cursor(SessionId(1)).unwrap(), Some(cursor(10)));
}

#[test]
fn saving_again_replaces_the_cursor() {
    let store = store_with_session(1);
    store.save_log_cursor(SessionId(1), &cursor(10)).unwrap();
    store.save_log_cursor(SessionId(1), &cursor(99)).unwrap();
    assert_eq!(store.log_cursor(SessionId(1)).unwrap(), Some(cursor(99)));
}

#[test]
fn missing_cursor_is_none_and_delete_is_idempotent() {
    let store = store_with_session(1);
    assert_eq!(store.log_cursor(SessionId(1)).unwrap(), None);
    store.delete_log_cursor(SessionId(1)).unwrap();
    store.save_log_cursor(SessionId(1), &cursor(5)).unwrap();
    store.delete_log_cursor(SessionId(1)).unwrap();
    assert_eq!(store.log_cursor(SessionId(1)).unwrap(), None);
}

#[test]
fn cursor_for_an_unknown_session_is_rejected() {
    let store = store_with_session(1);
    assert!(store.save_log_cursor(SessionId(2), &cursor(1)).is_err());
}
