//! Fake-PTY session manager behaviour tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::FakePty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig, SessionManagerError},
};
use protocol::{MachineId, SessionId, SessionState, Timestamp};
use std::{
    fs::{self, File},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "terminal-agent-session-{name}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

fn manager(name: &str) -> (SessionManager, Arc<FakePty>, PathBuf) {
    let data_dir = temp_dir(name);
    let backend = Arc::new(FakePty::new());
    let config = SessionManagerConfig {
        machine_id: MachineId("test-machine".into()),
        data_dir: data_dir.clone(),
        scrollback_lines: 3,
        clock: Arc::new(|| Timestamp(1_790_000_000_000)),
    };
    let manager = SessionManager::new(backend.clone(), config, AuditLog::new(&data_dir));
    (manager, backend, data_dir)
}

fn request() -> CreateSession {
    CreateSession {
        shell: Some("/bin/sh".into()),
        cwd: Some("/tmp".into()),
        cols: 80,
        rows: 24,
    }
}

async fn wait_for_state(manager: &SessionManager, id: SessionId, expected: SessionState) {
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if manager.session(id).unwrap().state == expected {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

async fn wait_for_scrollback(manager: &SessionManager, id: SessionId, expected: &str) {
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if manager
                .scrollback(id)
                .unwrap()
                .iter()
                .any(|line| line.contains(expected))
            {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn created_session_becomes_idle() {
    let (manager, fake, data_dir) = manager("create");

    let session = manager.create(request()).await.unwrap();

    assert_eq!(session.state, SessionState::Idle);
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn output_reaches_two_attached_clients() {
    let (manager, fake, data_dir) = manager("broadcast");
    let session = manager.create(request()).await.unwrap();
    let mut first = manager.attach(session.id).unwrap();
    let mut second = manager.attach(session.id).unwrap();

    fake.output(session.id, b"hello\r\n").unwrap();

    assert_eq!(first.output.recv().await.unwrap(), b"hello\r\n");
    assert_eq!(second.output.recv().await.unwrap(), b"hello\r\n");
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn detaching_every_client_leaves_session_running() {
    let (manager, fake, data_dir) = manager("detach");
    let session = manager.create(request()).await.unwrap();
    let first = manager.attach(session.id).unwrap();
    let second = manager.attach(session.id).unwrap();

    drop(first);
    drop(second);

    assert_eq!(
        manager.session(session.id).unwrap().state,
        SessionState::Idle
    );
    fake.output(session.id, b"still alive\n").unwrap();
    wait_for_scrollback(&manager, session.id, "still alive").await;
    assert_eq!(manager.scrollback(session.id).unwrap(), vec!["still alive"]);
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn killing_session_marks_it_interrupted() {
    let (manager, _fake, data_dir) = manager("kill");
    let session = manager.create(request()).await.unwrap();

    manager.kill(session.id).unwrap();

    assert_eq!(
        manager.session(session.id).unwrap().state,
        SessionState::Interrupted
    );
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn child_exit_zero_marks_session_completed() {
    let (manager, fake, data_dir) = manager("exit-zero");
    let session = manager.create(request()).await.unwrap();

    fake.exit(session.id, 0).unwrap();

    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn child_exit_nonzero_marks_session_failed() {
    let (manager, fake, data_dir) = manager("exit-nonzero");
    let session = manager.create(request()).await.unwrap();

    fake.exit(session.id, 3).unwrap();

    wait_for_state(&manager, session.id, SessionState::Failed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn resize_with_zero_dimension_is_rejected() {
    let (manager, fake, data_dir) = manager("resize-zero");
    let session = manager.create(request()).await.unwrap();

    let error = manager.resize(session.id, 0, 24).unwrap_err();

    assert!(matches!(
        error,
        SessionManagerError::InvalidTerminalSize { cols: 0, rows: 24 }
    ));
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn resize_updates_the_pty_dimensions() {
    let (manager, fake, data_dir) = manager("resize");
    let session = manager.create(request()).await.unwrap();

    manager.resize(session.id, 120, 40).unwrap();

    assert_eq!(fake.size(session.id).unwrap(), (120, 40));
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn attach_restores_text_drawn_on_row_five() {
    let (manager, fake, data_dir) = manager("screen");
    let session = manager.create(request()).await.unwrap();
    fake.output(session.id, b"\x1b[5;1Hrestored").unwrap();
    wait_for_scrollback(&manager, session.id, "restored").await;

    let attachment = manager.attach(session.id).unwrap();
    let mut restored = vt100::Parser::new(24, 80, 0);
    restored.process(&attachment.screen);

    assert_eq!(restored.screen().cell(4, 0).unwrap().contents(), "r");
    assert!(restored
        .screen()
        .rows(0, 8)
        .nth(4)
        .unwrap()
        .contains("restored"));
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn remote_input_is_written_and_audited_by_byte_count() {
    let (manager, fake, data_dir) = manager("input");
    let session = manager.create(request()).await.unwrap();

    manager
        .write_input(session.id, b"password=hunter2\n", "100.64.0.2:5000")
        .unwrap();

    assert_eq!(fake.input(session.id).unwrap(), b"password=hunter2\n");
    let audit = fs::read_to_string(data_dir.join("audit.jsonl")).unwrap();
    assert!(audit.contains("\"byte_count\":17"));
    assert!(!audit.contains("hunter2"));
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn raw_output_is_appended_to_a_compressed_session_log() {
    let (manager, fake, data_dir) = manager("compressed-log");
    let session = manager.create(request()).await.unwrap();
    fake.output(session.id, b"training output\r\n").unwrap();
    wait_for_scrollback(&manager, session.id, "training output").await;
    fake.exit(session.id, 0).unwrap();

    let log_path = data_dir.join("sessions/1.log.zst");
    let decoded = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if let Ok(file) = File::open(&log_path) {
                if let Ok(bytes) = zstd::stream::decode_all(file) {
                    if bytes == b"training output\r\n" {
                        return bytes;
                    }
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    assert_eq!(decoded, b"training output\r\n");
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}
