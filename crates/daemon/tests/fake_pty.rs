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
        sandbox: false,
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

#[tokio::test]
async fn input_that_cannot_be_audited_never_reaches_the_shell() {
    // AGENTS.md §5: every remote input is audit-logged. The audit used to be
    // written after the PTY write, so a failing audit log let input through.
    let fake = Arc::new(FakePty::echoing());
    let sessions = manager("audit-fails", fake.clone());
    let session = sessions.create(request()).await.unwrap();
    let mut attached = sessions.attach(session.id).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "sentinal-fake-pty-audit-fails-{}",
        std::process::id()
    ));
    let audit = dir.join("audit.jsonl");
    let _ = std::fs::remove_file(&audit);
    std::fs::create_dir_all(&audit).unwrap();

    assert!(sessions.write_input(session.id, b"a", "test").is_err());
    assert!(
        tokio::time::timeout(Duration::from_millis(100), attached.output.recv())
            .await
            .is_err(),
        "unaudited input reached the PTY"
    );
    fake.exit(session.id, 0).unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_history_attach_brings_back_lines_that_scrolled_off_the_screen() {
    let fake = Arc::new(FakePty::echoing());
    let sessions = manager("history", fake.clone());
    let session = sessions.create(request()).await.unwrap();
    let mut first = sessions.attach(session.id).unwrap();
    // Scrollback here keeps 3 lines; 30 lines push at least that many off 24 rows.
    for line in 1..=30 {
        sessions
            .write_input(session.id, format!("line {line}\r\n").as_bytes(), "test")
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), first.output.recv())
            .await
            .expect("an echo")
            .unwrap();
    }

    let plain = sessions.attach(session.id).unwrap();
    let with_history = sessions.attach_with_history(session.id, true).unwrap();

    let plain = String::from_utf8_lossy(&plain.screen).into_owned();
    let with_history = String::from_utf8_lossy(&with_history.screen).into_owned();
    assert!(!plain.contains("line 6"), "line 6 scrolled off the screen");
    assert!(with_history.contains("line 6"), "{with_history:?}");
    fake.exit(session.id, 0).unwrap();
}
