//! Fake-PTY session manager behaviour tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::{FakePty, PtyBackend, PtyError, PtySpawnRequest},
    session_manager::{CreateSession, SessionManager, SessionManagerConfig, SessionManagerError},
};
use protocol::{
    CaptureLevel, DaemonMessage, MachineId, ProcessTotals, Resource, Session, SessionId,
    SessionMetrics, SessionOrigin, SessionState, Timestamp,
};
use std::{
    fs::{self, File},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use store::Store;

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

fn stored_manager(name: &str) -> (SessionManager, Arc<FakePty>, Arc<Store>, PathBuf) {
    let data_dir = temp_dir(name);
    let backend = Arc::new(FakePty::new());
    let store = Arc::new(Store::open_in_memory().unwrap());
    let config = SessionManagerConfig {
        machine_id: MachineId("test-machine".into()),
        data_dir: data_dir.clone(),
        scrollback_lines: 3,
        clock: Arc::new(|| Timestamp(1_790_000_000_000)),
    };
    let manager = SessionManager::with_store(
        backend.clone(),
        config,
        AuditLog::new(&data_dir),
        store.clone(),
    )
    .unwrap();
    (manager, backend, store, data_dir)
}

fn manager_with_store(name: &str, store: Arc<Store>) -> (SessionManager, Arc<FakePty>, PathBuf) {
    let data_dir = temp_dir(name);
    let backend = Arc::new(FakePty::new());
    let config = SessionManagerConfig {
        machine_id: MachineId("test-machine".into()),
        data_dir: data_dir.clone(),
        scrollback_lines: 3,
        clock: Arc::new(|| Timestamp(1_790_000_000_000)),
    };
    let manager =
        SessionManager::with_store(backend.clone(), config, AuditLog::new(&data_dir), store)
            .unwrap();
    (manager, backend, data_dir)
}

/// A shell-integration signal tagged with the nonce the daemon gave this Session.
fn signal(fake: &FakePty, id: SessionId, payload: &str) -> String {
    let request = fake.spawn_request(id).unwrap();
    let (_, nonce) = request
        .env
        .iter()
        .find(|(key, _)| key == "SENTINAL_OSC_NONCE")
        .unwrap();
    format!("\x1b]{payload};sentinal={nonce}\x07")
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
async fn continuation_creates_a_distinct_safe_session_without_replaying_commands() {
    let (manager, fake, store, data_dir) = stored_manager("continue-safe");
    let mut create = request();
    create.title = Some("Stable title".into());
    let source = manager.create(create).await.unwrap();
    fake.output(source.id, b"untrusted-secret-output\r\n")
        .unwrap();
    wait_for_scrollback(&manager, source.id, "untrusted-secret-output").await;
    fake.exit(source.id, 0).unwrap();
    wait_for_state(&manager, source.id, SessionState::Completed).await;
    let before = store.session(source.id).unwrap().unwrap();

    let descendant = manager.continue_session(source.id).await.unwrap();

    assert_ne!(descendant.id, source.id);
    assert_eq!(descendant.continued_from, Some(source.id));
    assert_eq!(descendant.origin, SessionOrigin::Ui);
    assert_eq!(descendant.capture, CaptureLevel::Full);
    assert_eq!(descendant.title.as_deref(), Some("Stable title"));
    assert_eq!(descendant.shell, "/bin/sh");
    assert_eq!(descendant.cwd, "/tmp");
    assert_eq!(descendant.current_command, None);
    assert_eq!(descendant.exit_code, None);
    assert_eq!(fake.size(descendant.id).unwrap(), (80, 24));
    assert!(fake.input(descendant.id).unwrap().is_empty());
    assert!(!descendant
        .title
        .as_deref()
        .unwrap_or_default()
        .contains("secret"));
    assert!(!data_dir.join("audit.jsonl").exists());
    assert_eq!(store.session(source.id).unwrap().unwrap(), before);
    assert_eq!(
        store.direct_successor(source.id).unwrap().unwrap().id,
        descendant.id
    );

    fake.exit(descendant.id, 0).unwrap();
    wait_for_state(&manager, descendant.id, SessionState::Completed).await;
    let replay = manager.replay_output(descendant.id, 0, 1024).unwrap();
    assert!(replay.bytes.is_empty());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn simultaneous_continuations_return_one_successor() {
    let (manager, fake, store, data_dir) = stored_manager("continue-concurrent");
    let source = manager.create(request()).await.unwrap();
    fake.exit(source.id, 3).unwrap();
    wait_for_state(&manager, source.id, SessionState::Failed).await;

    let first_manager = manager.clone();
    let second_manager = manager.clone();
    let (first, second) = tokio::join!(
        first_manager.continue_session(source.id),
        second_manager.continue_session(source.id)
    );
    let first = first.unwrap();
    let second = second.unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(store.ordered_lineage(source.id).unwrap().len(), 2);
    assert_eq!(manager.list().unwrap().len(), 2);
    fake.exit(first.id, 0).unwrap();
    wait_for_state(&manager, first.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn persisted_finished_session_can_continue_after_restart_with_safe_metadata() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut source = Session::new(
        SessionId(40),
        MachineId("test-machine".into()),
        "/bin/zsh",
        "/work/project",
        Timestamp(100),
    );
    source.title = None;
    source.project_id = Some("project-a".into());
    source
        .transition(SessionState::Idle, Timestamp(101))
        .unwrap();
    source
        .transition(SessionState::Interrupted, Timestamp(200))
        .unwrap();
    store.insert_session(&source).unwrap();
    let (manager, fake, data_dir) = manager_with_store("continue-restart", store.clone());

    let descendant = manager.continue_session(source.id).await.unwrap();

    assert_eq!(descendant.title.as_deref(), Some("Session 40"));
    assert_eq!(descendant.project_id, source.project_id);
    assert_eq!(descendant.shell, source.shell);
    assert_eq!(descendant.cwd, source.cwd);
    assert_eq!(store.ordered_lineage(source.id).unwrap().len(), 2);
    fake.exit(descendant.id, 0).unwrap();
    wait_for_state(&manager, descendant.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn continuation_rejects_live_and_reduced_capture_sources_before_spawn() {
    let (manager, fake, _store, data_dir) = stored_manager("continue-reject");
    let live = manager.create(request()).await.unwrap();
    let live_error = manager.continue_session(live.id).await.unwrap_err();
    assert!(live_error.to_string().contains("Idle"));
    assert!(matches!(
        fake.spawn_request(SessionId(live.id.0 + 1)),
        Err(PtyError::UnknownSession(_))
    ));
    fake.exit(live.id, 0).unwrap();
    wait_for_state(&manager, live.id, SessionState::Completed).await;

    let reduced_store = Arc::new(Store::open_in_memory().unwrap());
    let mut reduced = Session::new(
        SessionId(8),
        MachineId("test-machine".into()),
        "/bin/sh",
        "/work",
        Timestamp(1),
    );
    reduced.origin = SessionOrigin::ShellHook;
    reduced.capture = CaptureLevel::Commands;
    reduced
        .transition(SessionState::Idle, Timestamp(2))
        .unwrap();
    reduced
        .transition(SessionState::Interrupted, Timestamp(3))
        .unwrap();
    reduced_store.insert_session(&reduced).unwrap();
    let (reduced_manager, reduced_fake, reduced_dir) =
        manager_with_store("continue-reduced", reduced_store);
    let reduced_error = reduced_manager
        .continue_session(reduced.id)
        .await
        .unwrap_err();
    assert!(reduced_error.to_string().contains("COMMANDS"));
    assert!(matches!(
        reduced_fake.spawn_request(SessionId(9)),
        Err(PtyError::UnknownSession(_))
    ));

    fs::remove_dir_all(data_dir).unwrap();
    assert!(!reduced_dir.exists());
}

#[tokio::test]
async fn continuation_store_failure_kills_staged_pty_and_leaves_no_log_or_handle() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut source = Session::new(
        SessionId(1),
        MachineId("test-machine".into()),
        "/bin/sh",
        "/work",
        Timestamp(1),
    );
    source.transition(SessionState::Idle, Timestamp(2)).unwrap();
    source
        .transition(SessionState::Completed, Timestamp(3))
        .unwrap();
    source.exit_code = Some(0);
    store.insert_session(&source).unwrap();
    let (manager, fake, data_dir) = manager_with_store("continue-store-failure", store.clone());
    // The manager reserved ID 2 at construction. A competing persisted row
    // forces the real SQLite insert to fail after the PTY has been staged.
    store
        .insert_session(&Session::new(
            SessionId(2),
            MachineId("test-machine".into()),
            "/bin/sh",
            "/other",
            Timestamp(10),
        ))
        .unwrap();

    assert!(manager.continue_session(source.id).await.is_err());
    assert_eq!(fake.operation_counts(SessionId(2)).unwrap().kills, 1);
    assert!(matches!(
        manager.session(SessionId(2)),
        Err(SessionManagerError::UnknownSession(_))
    ));
    assert!(!data_dir.join("sessions/2.log.zst").exists());
    assert_eq!(store.session(source.id).unwrap().unwrap(), source);
    fs::remove_dir_all(data_dir).unwrap();
}

#[derive(Debug)]
struct FailingPty;

impl PtyBackend for FailingPty {
    fn spawn(&self, request: &PtySpawnRequest) -> Result<daemon::pty::SpawnedPty, PtyError> {
        Err(PtyError::Operation {
            operation: "spawn",
            session_id: request.session_id,
            message: "injected failure".into(),
        })
    }
}

#[tokio::test]
async fn continuation_pty_failure_leaves_source_and_store_unchanged() {
    let data_dir = temp_dir("continue-pty-failure");
    let store = Arc::new(Store::open_in_memory().unwrap());
    let mut source = Session::new(
        SessionId(1),
        MachineId("test-machine".into()),
        "/bin/sh",
        "/work",
        Timestamp(1),
    );
    source.transition(SessionState::Idle, Timestamp(2)).unwrap();
    source
        .transition(SessionState::Interrupted, Timestamp(3))
        .unwrap();
    store.insert_session(&source).unwrap();
    let manager = SessionManager::with_store(
        Arc::new(FailingPty),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 3,
            clock: Arc::new(|| Timestamp(10)),
        },
        AuditLog::new(&data_dir),
        store.clone(),
    )
    .unwrap();

    assert!(manager.continue_session(source.id).await.is_err());
    assert_eq!(store.sessions().unwrap(), vec![source]);
    assert!(!data_dir.join("sessions/2.log.zst").exists());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn created_session_becomes_idle() {
    let (manager, fake, data_dir) = manager("create");

    let session = manager.create(request()).await.unwrap();

    assert_eq!(session.state, SessionState::Idle);
    assert_eq!(session.origin, SessionOrigin::Ui);
    assert_eq!(session.capture, CaptureLevel::Full);
    assert_eq!(session.pid, Some(10_001));
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    assert_eq!(manager.session(session.id).unwrap().exit_code, Some(0));
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn session_starts_when_shell_hook_installation_fails() {
    let (manager, fake, data_dir) = manager("hook-failure");
    fs::create_dir_all(&data_dir).unwrap();
    fs::write(data_dir.join("shell"), b"not a directory").unwrap();

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
    assert_eq!(manager.session(session.id).unwrap().exit_code, Some(3));
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
async fn every_finished_state_rejects_mutation_before_touching_the_pty_or_audit() {
    for (name, exit_code, expected) in [
        ("completed", Some(0), SessionState::Completed),
        ("failed", Some(3), SessionState::Failed),
        ("interrupted", None, SessionState::Interrupted),
    ] {
        let (manager, fake, data_dir) = manager(name);
        let session = manager.create(request()).await.unwrap();
        if let Some(code) = exit_code {
            fake.exit(session.id, code).unwrap();
            wait_for_state(&manager, session.id, expected).await;
        } else {
            manager.kill(session.id).unwrap();
        }
        let before_counts = fake.operation_counts(session.id).unwrap();

        let input = manager
            .write_input(session.id, b"never-store-this-secret", "100.64.0.2:5000")
            .unwrap_err();
        let resize = manager.resize(session.id, 120, 40).unwrap_err();
        let kill = manager.kill(session.id).unwrap_err();

        for (operation, error) in [
            ("accept terminal input", input),
            ("resize", resize),
            ("kill a daemon-owned PTY", kill),
        ] {
            assert!(matches!(
                error,
                SessionManagerError::TerminalStateDenied {
                    session_id,
                    state,
                    operation: rejected,
                } if session_id == session.id && state == expected && rejected == operation
            ));
            let text = error.to_string();
            assert!(text.contains(&session.id.0.to_string()), "{text}");
            assert!(text.contains(&format!("{expected:?}")), "{text}");
            assert!(!text.contains("os error"), "{text}");
        }
        assert_eq!(fake.operation_counts(session.id).unwrap(), before_counts);
        assert_eq!(fake.input(session.id).unwrap(), b"");
        assert!(!data_dir.join("audit.jsonl").exists());

        let attachment = manager.attach(session.id).unwrap();
        assert!(attachment.output.is_empty());
        fs::remove_dir_all(data_dir).unwrap();
    }
}

#[tokio::test]
async fn closed_pty_write_race_reconciles_once_and_returns_a_stable_error() {
    let (manager, fake, data_dir) = manager("closed-write-race");
    let session = manager.create(request()).await.unwrap();
    fake.exit(session.id, 0).unwrap();

    let first = manager
        .write_input(session.id, b"secret-race-input", "100.64.0.2:5000")
        .unwrap_err();
    let second = manager
        .write_input(session.id, b"secret-race-input", "100.64.0.2:5000")
        .unwrap_err();

    for error in [first, second] {
        assert!(matches!(
            error,
            SessionManagerError::TerminalStateDenied {
                session_id,
                state: SessionState::Completed,
                operation: "accept terminal input",
            } if session_id == session.id
        ));
        assert!(!error.to_string().contains("os error"));
    }
    assert!(fake.operation_counts(session.id).unwrap().writes <= 1);
    assert_eq!(fake.input(session.id).unwrap(), b"");
    assert!(!data_dir.join("audit.jsonl").exists());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn rejected_finished_input_is_absent_from_audit_store_and_output_log() {
    let (manager, fake, store, data_dir) = stored_manager("finished-input-privacy");
    let session = manager.create(request()).await.unwrap();
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;

    manager
        .write_input(session.id, b"unique-finished-secret", "100.64.0.2:5000")
        .unwrap_err();

    assert!(!data_dir.join("audit.jsonl").exists());
    let stored = serde_json::to_vec(&store.session(session.id).unwrap()).unwrap();
    assert!(!stored
        .windows(b"unique-finished-secret".len())
        .any(|window| window == b"unique-finished-secret"));
    assert!(store.history(Some(session.id), 10).unwrap().is_empty());
    let log_path = data_dir.join("sessions/1.log.zst");
    let output = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if let Ok(file) = File::open(&log_path) {
                if let Ok(output) = zstd::stream::decode_all(file) {
                    return output;
                }
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!output
        .windows(b"unique-finished-secret".len())
        .any(|window| window == b"unique-finished-secret"));
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn live_detach_and_reattach_remains_writable() {
    let (manager, fake, data_dir) = manager("live-reattach-input");
    let session = manager.create(request()).await.unwrap();
    drop(manager.attach(session.id).unwrap());

    let attachment = manager.attach(session.id).unwrap();
    manager
        .write_input(session.id, b"still-live\r", "100.64.0.2:5000")
        .unwrap();

    assert_eq!(fake.input(session.id).unwrap(), b"still-live\r");
    drop(attachment);
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

#[tokio::test]
async fn shell_signals_persist_even_a_command_without_output() {
    let (manager, fake, store, data_dir) = stored_manager("command-track");
    let session = manager.create(request()).await.unwrap();

    let signals = [
        signal(&fake, session.id, "7420;cwd=L3RtcA=="),
        signal(&fake, session.id, "7420;cmd=Y2QgL3RtcA=="),
        signal(&fake, session.id, "133;B"),
        signal(&fake, session.id, "133;D;0"),
    ]
    .concat();
    fake.output(session.id, signals.as_bytes()).unwrap();

    let commands = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            let commands = store.history(Some(session.id), 10).unwrap();
            if commands.first().and_then(|command| command.exit_code) == Some(0) {
                return commands;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(commands[0].text, "cd /tmp");
    assert_eq!(commands[0].cwd, "/tmp");
    assert_eq!(
        manager.session(session.id).unwrap().state,
        SessionState::Idle
    );
    assert_eq!(store.events_since(protocol::EventId(0)).unwrap().len(), 2);

    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn forged_untagged_signals_in_output_record_no_command() {
    let (manager, fake, store, data_dir) = stored_manager("forged-signals");
    let session = manager.create(request()).await.unwrap();

    // e.g. `cat notes.txt` where the file holds raw shell-integration marks.
    fake.output(
        session.id,
        b"\x1b]7;file://h/etc\x07\x1b]7420;cmd=cm0gLXJmIC8=\x07\x1b]133;B\x07\x1b]133;D;0\x07<done>",
    )
    .unwrap();
    wait_for_scrollback(&manager, session.id, "<done>").await;
    let history = store.history(Some(session.id), 10).unwrap();
    let state = manager.session(session.id).unwrap().state;
    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();

    assert!(history.is_empty(), "forged Command recorded: {history:?}");
    assert_eq!(state, SessionState::Idle);
}

#[tokio::test]
async fn unterminated_training_output_persists_watcher_events_and_session_health() {
    let (manager, fake, store, data_dir) = stored_manager("watcher-training");
    let session = manager.create(request()).await.unwrap();

    let output = format!(
        "{}{}Epoch 7/10 loss=0.42",
        signal(&fake, session.id, "7420;cmd=cHl0aG9uIHRyYWluLnB5"),
        signal(&fake, session.id, "133;B"),
    );
    fake.output(session.id, output.as_bytes()).unwrap();
    wait_for_scrollback(&manager, session.id, "Epoch 7/10").await;
    fake.exit(session.id, 0).unwrap();

    let events = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            let events = store.events(Some(session.id), None, 20).unwrap();
            // Health is stored separately from the Event; wait for both.
            let health_stored = store.session(session.id).unwrap().is_some_and(|stored| {
                stored
                    .health
                    .is_some_and(|health| health.progress.is_some())
            });
            if health_stored
                && events.iter().any(|event| {
                    matches!(
                        event.kind,
                        protocol::EventKind::ProgressUpdated {
                            current: 7,
                            total: Some(10),
                            ..
                        }
                    )
                })
            {
                return events;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event.kind, protocol::EventKind::ProgressUpdated { .. })));
    let stored = store.session(session.id).unwrap().unwrap();
    assert_eq!(
        stored.health.as_ref().and_then(|health| health.progress),
        Some(70.0)
    );
    assert_eq!(
        stored.training.as_ref().and_then(|training| training.loss),
        Some(0.42)
    );

    wait_for_state(&manager, session.id, SessionState::Completed).await;
    let stored = store.session(session.id).unwrap().unwrap();
    assert_eq!(stored.current_command, None);
    let commands = store.history(Some(session.id), 10).unwrap();
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].exit_code, Some(0));
    assert!(commands[0].finished_at.is_some());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn attached_client_receives_metrics_and_resource_events() {
    let (manager, fake, store, data_dir) = stored_manager("metric-stream");
    let session = manager.create(request()).await.unwrap();
    let mut attachment = manager.attach(session.id).unwrap();
    let metrics = SessionMetrics {
        session_id: session.id,
        at: Timestamp(1_790_000_000_100),
        totals: ProcessTotals {
            cpu_percent: 1.0,
            rss_bytes: 512,
            gpu_mem_bytes: 256,
        },
    };

    manager
        .publish_metrics_with_resources(
            metrics.clone(),
            watcher::MetricsObservation {
                cpu_percent: 1.0,
                cpu_machine_percent: Some(0.1),
                gpu_percent: Some(96.0),
                ram_percent: Some(50.0),
                vram_percent: Some(25.0),
            },
        )
        .unwrap();
    let received_metrics = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if let DaemonMessage::SessionMetrics { metrics } =
                attachment.structured.recv().await.unwrap()
            {
                return metrics;
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(received_metrics, metrics);
    assert_eq!(
        manager
            .session(session.id)
            .unwrap()
            .health
            .unwrap()
            .resources,
        protocol::HealthLevel::Critical
    );

    manager
        .publish_resource_high(
            session.id,
            Resource::Cpu,
            96.0,
            Timestamp(1_790_000_000_100),
        )
        .unwrap();
    let event = tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if let DaemonMessage::Event { event } = attachment.structured.recv().await.unwrap() {
                return event;
            }
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        event.kind,
        protocol::EventKind::ResourceHigh {
            resource: Resource::Cpu,
            percent: 96.0
        }
    ));
    assert_eq!(store.events_since(protocol::EventId(0)).unwrap().len(), 1);

    fake.exit(session.id, 0).unwrap();
    wait_for_state(&manager, session.id, SessionState::Completed).await;
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn machine_metrics_reach_clients_without_an_attachment() {
    let (manager, _fake, _data_dir) = manager("machine-metric-observer");
    let mut observer = manager.observe();
    let metrics = protocol::MachineMetrics {
        at: Timestamp(1_790_000_000_100),
        cpu_percent: 42.0,
        ram_used_bytes: 512,
        ram_total_bytes: 1024,
        gpus: Vec::new(),
    };

    manager.publish_machine_metrics(metrics.clone()).unwrap();

    assert!(matches!(
        observer.try_recv().unwrap(),
        DaemonMessage::MachineMetrics { metrics: received } if received == metrics
    ));
}

#[tokio::test]
async fn shutdown_interrupts_every_live_pty_session_and_skips_finished_ones() {
    let (manager, fake, store, data_dir) = stored_manager("shutdown");
    let live_a = manager.create(request()).await.unwrap();
    let live_b = manager.create(request()).await.unwrap();
    let done = manager.create(request()).await.unwrap();
    fake.exit(done.id, 0).unwrap();
    wait_for_state(&manager, done.id, SessionState::Completed).await;

    let interrupted = manager.shutdown();

    assert_eq!(interrupted, 2);
    for id in [live_a.id, live_b.id] {
        assert_eq!(fake.operation_counts(id).unwrap().kills, 1, "{id}");
        assert_eq!(
            manager.session(id).unwrap().state,
            SessionState::Interrupted
        );
        assert_eq!(
            store.session(id).unwrap().unwrap().state,
            SessionState::Interrupted
        );
    }
    assert_eq!(fake.operation_counts(done.id).unwrap().kills, 0);
    assert_eq!(
        manager.session(done.id).unwrap().state,
        SessionState::Completed
    );
    assert_eq!(
        manager.shutdown(),
        0,
        "a second shutdown has nothing left to stop"
    );
    fs::remove_dir_all(data_dir).unwrap();
}
