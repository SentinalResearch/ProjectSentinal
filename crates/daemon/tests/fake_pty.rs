//! The fake PTY's opt-in echo (`TA_FAKE_PTY=echo`) used by browser tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::FakePty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{MachineId, SessionOrigin, Timestamp};
use std::{sync::Arc, time::Duration};

fn manager(name: &str, backend: Arc<FakePty>) -> SessionManager {
    let dir = std::env::temp_dir().join(format!("sentinal-fake-pty-{name}-{}", std::process::id()));
    SessionManager::new(
        backend,
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: dir.clone(),
            scrollback_lines: 3,
            clock: Arc::new(|| Timestamp(1_790_000_000_000)),
        },
        AuditLog::new(&dir),
    )
}

fn request() -> CreateSession {
    CreateSession {
        shell: Some("/bin/sh".into()),
        cwd: Some("/tmp".into()),
        cols: 80,
        rows: 24,
        origin: SessionOrigin::Ui,
        title: None,
        command: None,
        env: None,
    }
}

#[tokio::test]
async fn an_echoing_fake_writes_input_back() {
    let fake = Arc::new(FakePty::echoing());
    let sessions = manager("echo", fake.clone());
    let session = sessions.create(request()).await.unwrap();
    let mut attached = sessions.attach(session.id).unwrap();
    sessions.write_input(session.id, b"a", "test").unwrap();
    let echoed = tokio::time::timeout(Duration::from_secs(5), attached.output.recv())
        .await
        .expect("an echo")
        .unwrap();
    assert_eq!(echoed, b"a");
    fake.exit(session.id, 0).unwrap();
}

#[tokio::test]
async fn the_default_fake_does_not_echo() {
    let fake = Arc::new(FakePty::new());
    let sessions = manager("quiet", fake.clone());
    let session = sessions.create(request()).await.unwrap();
    let mut attached = sessions.attach(session.id).unwrap();
    sessions.write_input(session.id, b"a", "test").unwrap();
    assert!(
        tokio::time::timeout(Duration::from_millis(100), attached.output.recv())
            .await
            .is_err()
    );
    fake.exit(session.id, 0).unwrap();
}
