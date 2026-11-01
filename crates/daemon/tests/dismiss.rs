//! A failed Session can be dismissed from "Needs you", on every device.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{MachineId, ProjectedSession, Session, SessionId, SessionState, Timestamp};
use std::sync::Arc;
use store::Store;

const NOW: i64 = 1_790_000_000_000;

fn session(id: u64, state: SessionState) -> Session {
    let mut session = Session::new(
        SessionId(id),
        MachineId("m".into()),
        "/bin/sh",
        "/",
        Timestamp(1),
    );
    session
        .transition(SessionState::Idle, Timestamp(1))
        .unwrap();
    session
        .transition(SessionState::Running, Timestamp(2))
        .unwrap();
    if state != SessionState::Running {
        session.transition(state, Timestamp(3)).unwrap();
    }
    session
}

fn manager(store: Arc<Store>) -> SessionManager {
    let dir = std::env::temp_dir().join(format!("sentinal-dismiss-{}", std::process::id()));
    SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("m".into()),
            data_dir: dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(NOW)),
        },
        AuditLog::new(&dir),
        store,
    )
    .unwrap()
}

#[test]
fn a_failed_session_is_dismissed_and_stays_dismissed() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    store
        .insert_session(&session(1, SessionState::Failed))
        .unwrap();
    let manager = manager(store.clone());

    let dismissed = manager.dismiss(SessionId(1)).unwrap();
    assert_eq!(dismissed.dismissed_at, Some(Timestamp(NOW)));
    // Kept with the Session, and sent to the control's dashboard copy.
    assert_eq!(
        store.session(SessionId(1)).unwrap().unwrap().dismissed_at,
        Some(Timestamp(NOW))
    );
    assert_eq!(
        ProjectedSession::from_session(&dismissed).dismissed_at,
        Some(Timestamp(NOW))
    );
    // Dismissing again changes nothing.
    assert_eq!(
        manager.dismiss(SessionId(1)).unwrap().dismissed_at,
        Some(Timestamp(NOW))
    );
}

#[test]
fn only_failed_sessions_can_be_dismissed() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    store
        .insert_session(&session(1, SessionState::Completed))
        .unwrap();
    store
        .insert_session(&session(2, SessionState::Interrupted))
        .unwrap();
    let manager = manager(store);
    // A waiting prompt or a stall still needs a person; only failures are dismissed.
    assert!(manager.dismiss(SessionId(1)).is_err());
    assert!(manager.dismiss(SessionId(2)).is_err());
    assert!(manager.dismiss(SessionId(9)).is_err());
}
