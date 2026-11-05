//! Finished Sessions can be hidden, restored, and permanently deleted.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{MachineId, Session, SessionId, SessionState, Timestamp};
use std::{path::PathBuf, sync::Arc};
use store::Store;

const NOW: i64 = 1_790_000_000_000;

fn session(id: u64, state: SessionState, from: Option<u64>) -> Session {
    let mut session = Session::new(
        SessionId(id),
        MachineId("m".into()),
        "/bin/sh",
        "/work",
        Timestamp(1),
    );
    session.state = state;
    session.continued_from = from.map(SessionId);
    session
}

fn setup(name: &str) -> (Arc<Store>, SessionManager, PathBuf) {
    let dir = std::env::temp_dir().join(format!("sentinal-remove-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("sessions")).unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let manager = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("m".into()),
            data_dir: dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(NOW)),
        },
        AuditLog::new(&dir),
        store.clone(),
    )
    .unwrap();
    (store, manager, dir)
}

#[test]
fn every_finished_state_can_be_hidden_and_restored() {
    let (store, manager, _) = setup("hide");
    for (id, state) in [
        (1, SessionState::Completed),
        (2, SessionState::Failed),
        (3, SessionState::Interrupted),
    ] {
        store.insert_session(&session(id, state, None)).unwrap();
        let hidden = manager.dismiss(SessionId(id)).unwrap();
        assert_eq!(hidden.dismissed_at, Some(Timestamp(NOW)));
        assert_eq!(
            store.session(SessionId(id)).unwrap().unwrap().dismissed_at,
            Some(Timestamp(NOW))
        );
        assert_eq!(manager.restore(SessionId(id)).unwrap().dismissed_at, None);
        assert_eq!(
            store.session(SessionId(id)).unwrap().unwrap().dismissed_at,
            None
        );
    }
}

#[test]
fn a_live_session_cannot_be_hidden_or_deleted() {
    let (store, manager, _) = setup("live");
    store
        .insert_session(&session(1, SessionState::Running, None))
        .unwrap();
    assert!(manager.dismiss(SessionId(1)).is_err());
    let error = manager.delete(SessionId(1)).unwrap_err().to_string();
    assert!(error.contains("stop it first"), "{error}");
    assert!(store.session(SessionId(1)).unwrap().is_some());
}

#[test]
fn a_continued_chain_is_hidden_and_deleted_as_one() {
    let (store, manager, dir) = setup("chain");
    store
        .insert_session(&session(1, SessionState::Completed, None))
        .unwrap();
    store
        .insert_continuation(SessionId(1), &session(2, SessionState::Failed, Some(1)))
        .unwrap();
    let log = |id: u64| dir.join("sessions").join(format!("{id}.log.zst"));
    std::fs::write(log(1), b"old").unwrap();
    std::fs::write(log(2), b"new").unwrap();

    manager.dismiss(SessionId(2)).unwrap();
    assert!(store
        .session(SessionId(1))
        .unwrap()
        .unwrap()
        .dismissed_at
        .is_some());

    let deleted = manager.delete(SessionId(1)).unwrap();
    assert_eq!(deleted, vec![SessionId(2), SessionId(1)]);
    assert!(store.sessions().unwrap().is_empty());
    assert!(manager.list().unwrap().is_empty());
    assert!(!log(1).exists() && !log(2).exists());
}

#[test]
fn a_chain_with_a_live_generation_stays() {
    let (store, manager, _) = setup("chain-live");
    store
        .insert_session(&session(1, SessionState::Completed, None))
        .unwrap();
    store
        .insert_continuation(SessionId(1), &session(2, SessionState::Running, Some(1)))
        .unwrap();
    assert!(manager.delete(SessionId(1)).is_err());
    assert!(manager.dismiss(SessionId(1)).is_err());
    assert_eq!(store.sessions().unwrap().len(), 2);
}

#[test]
fn a_missing_log_does_not_block_deletion() {
    let (store, manager, _) = setup("nolog");
    store
        .insert_session(&session(1, SessionState::Completed, None))
        .unwrap();
    assert_eq!(manager.delete(SessionId(1)).unwrap(), vec![SessionId(1)]);
}
