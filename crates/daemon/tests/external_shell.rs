//! SHELL_HOOK / COMMANDS Sessions reported through the local socket path.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    local_socket::{handle_connection, SecurityAudit},
    process_probe::{ProcessIdentity, ProcessProbe},
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig, SessionManagerError},
};
use protocol::{
    CaptureLevel, CommandStatus, Envelope, EventKind, LocalErrorCode, LocalRequest, LocalResponse,
    MachineId, SessionId, SessionOrigin, SessionState, Timestamp,
};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use store::Store;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const OWNER: u32 = 501;
const SHELL_PID: u32 = 4_242;
const SHELL_STARTED: Timestamp = Timestamp(1_789_999_990_000);

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

type Processes = Arc<Mutex<HashMap<u32, ProcessIdentity>>>;

struct Harness {
    manager: SessionManager,
    processes: Processes,
    store: Arc<Store>,
    data_dir: PathBuf,
}

fn probe(processes: &Processes) -> ProcessProbe {
    let processes = processes.clone();
    ProcessProbe::new(OWNER, move |pid| {
        processes.lock().unwrap().get(&pid).copied()
    })
}

fn harness_with(store: Arc<Store>, data_dir: PathBuf, processes: Processes) -> Harness {
    let manager = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(1_790_000_000_000)),
        },
        AuditLog::new(&data_dir),
        store.clone(),
    )
    .unwrap()
    .with_process_probe(probe(&processes));
    Harness {
        manager,
        processes,
        store,
        data_dir,
    }
}

fn harness() -> Harness {
    let data_dir = std::env::temp_dir().join(format!(
        "sentinal-external-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&data_dir).unwrap();
    let store = Arc::new(Store::open(&data_dir.join("sentinal.sqlite3")).unwrap());
    let processes: Processes = Arc::new(Mutex::new(HashMap::from([(
        SHELL_PID,
        ProcessIdentity {
            started_at: SHELL_STARTED,
            uid: OWNER,
        },
    )])));
    harness_with(store, data_dir, processes)
}

fn register(h: &Harness) -> protocol::Session {
    h.manager
        .register_shell(
            SHELL_PID,
            "/bin/zsh",
            "/work",
            Some("Ordinary shell".into()),
        )
        .unwrap()
}

fn start(h: &Harness, id: SessionId, sequence: u64, text: &str, at: i64) -> protocol::Command {
    h.manager
        .shell_command_started(id, SHELL_PID, sequence, text, "/work/src", Timestamp(at))
        .unwrap()
}

fn finish(
    h: &Harness,
    id: SessionId,
    sequence: u64,
    exit: i32,
    at: i64,
) -> Option<protocol::Command> {
    h.manager
        .shell_command_finished(id, SHELL_PID, sequence, exit, Timestamp(at))
        .unwrap()
}

// ---- registration ----------------------------------------------------------

#[test]
fn registration_creates_one_commands_session_rooted_at_the_shell() {
    let h = harness();
    let session = register(&h);

    assert_eq!(session.origin, SessionOrigin::ShellHook);
    assert_eq!(session.capture, CaptureLevel::Commands);
    assert_eq!(session.pid, Some(SHELL_PID));
    assert_eq!(session.process_started_at, Some(SHELL_STARTED));
    assert_eq!(session.shell, "/bin/zsh");
    assert_eq!(session.cwd, "/work");
    assert_eq!(session.title.as_deref(), Some("Ordinary shell"));
    assert_eq!(session.state, SessionState::Idle);
    assert_eq!(h.store.session(session.id).unwrap().unwrap(), session);
    assert_eq!(
        h.manager.process_roots().unwrap(),
        vec![(session.id, SHELL_PID)]
    );
}

#[test]
fn repeated_registration_of_the_same_shell_is_idempotent() {
    let h = harness();
    let first = register(&h);
    let second = register(&h);
    assert_eq!(first.id, second.id);
    assert_eq!(h.manager.list().unwrap().len(), 1);
}

#[test]
fn a_reused_pid_registers_a_new_session() {
    let h = harness();
    let first = register(&h);
    h.processes.lock().unwrap().insert(
        SHELL_PID,
        ProcessIdentity {
            started_at: Timestamp(SHELL_STARTED.0 + 60_000),
            uid: OWNER,
        },
    );
    let second = register(&h);
    assert_ne!(first.id, second.id);
}

#[test]
fn unknown_or_foreign_processes_cannot_be_registered() {
    let h = harness();
    let error = h
        .manager
        .register_shell(9_999, "/bin/zsh", "/work", None)
        .unwrap_err();
    assert!(error.to_string().contains("9999"), "{error}");

    h.processes.lock().unwrap().insert(
        77,
        ProcessIdentity {
            started_at: SHELL_STARTED,
            uid: 0,
        },
    );
    let error = h
        .manager
        .register_shell(77, "/bin/zsh", "/work", None)
        .unwrap_err();
    assert!(error.to_string().contains("uid 0"), "{error}");
    assert!(h.manager.list().unwrap().is_empty());
}

// ---- Commands --------------------------------------------------------------

#[test]
fn commands_are_stored_once_with_exact_text_cwd_exit_and_duration() {
    let h = harness();
    let session = register(&h);
    let text = "printf '%s' \"a;b $(x)\" | grep -v y";

    let running = start(&h, session.id, 1, text, 1_790_000_001_000);
    assert_eq!(running.text, text);
    assert_eq!(running.cwd, "/work/src");
    assert_eq!(
        h.manager.session(session.id).unwrap().state,
        SessionState::Running
    );

    let done = finish(&h, session.id, 1, 3, 1_790_000_001_250).unwrap();
    assert_eq!(done.exit_code, Some(3));
    assert_eq!(done.status(), CommandStatus::Failed);
    assert_eq!(done.duration(Timestamp(0)).0, 250);

    // A zero-output Command counts like any other.
    start(&h, session.id, 2, "true", 1_790_000_002_000);
    finish(&h, session.id, 2, 0, 1_790_000_002_001).unwrap();

    let history = h.store.history(Some(session.id), 10).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history.iter().filter(|c| c.text == text).count(), 1);
    assert_eq!(
        h.manager.session(session.id).unwrap().state,
        SessionState::Idle
    );
}

#[test]
fn finish_that_arrives_before_its_start_is_applied_when_the_start_arrives() {
    let h = harness();
    let session = register(&h);

    assert!(finish(&h, session.id, 1, 1, 1_790_000_001_005).is_none());
    let command = start(&h, session.id, 1, "false", 1_790_000_001_000);
    assert_eq!(command.exit_code, Some(1));
    assert_eq!(command.finished_at, Some(Timestamp(1_790_000_001_005)));
    assert_eq!(
        h.manager.session(session.id).unwrap().state,
        SessionState::Idle
    );
}

#[test]
fn a_finish_stamped_before_its_start_is_clamped_to_zero_duration() {
    let h = harness();
    let session = register(&h);
    start(&h, session.id, 1, "true", 1_790_000_001_010);
    let done = finish(&h, session.id, 1, 0, 1_790_000_001_000).unwrap();
    assert_eq!(done.finished_at, Some(Timestamp(1_790_000_001_010)));
}

#[test]
fn duplicate_or_stale_sequences_are_conflicts_and_store_nothing_new() {
    let h = harness();
    let session = register(&h);
    start(&h, session.id, 1, "sleep 1", 1_790_000_001_000);
    finish(&h, session.id, 1, 0, 1_790_000_002_000).unwrap();

    let again =
        h.manager
            .shell_command_started(session.id, SHELL_PID, 1, "sleep 1", "/w", Timestamp(1));
    assert!(
        again.is_ok(),
        "a repeated start returns the recorded Command"
    );
    let stale = h
        .manager
        .shell_command_finished(session.id, SHELL_PID, 1, 0, Timestamp(1_790_000_003_000))
        .unwrap_err();
    assert!(stale.to_string().contains("sequence 1"), "{stale}");
    assert_eq!(h.store.history(Some(session.id), 10).unwrap().len(), 1);
}

#[test]
fn a_lost_finish_does_not_block_the_next_command() {
    let h = harness();
    let session = register(&h);
    start(&h, session.id, 1, "vim notes", 1_790_000_001_000);
    start(&h, session.id, 2, "ls", 1_790_000_009_000);
    let done = finish(&h, session.id, 2, 0, 1_790_000_009_010).unwrap();
    assert_eq!(done.text, "ls");
    assert_eq!(h.store.history(Some(session.id), 10).unwrap().len(), 2);
}

#[test]
fn reports_for_the_wrong_pid_or_a_full_session_are_rejected() {
    let h = harness();
    let session = register(&h);
    let error = h
        .manager
        .shell_command_started(session.id, 1, 1, "ls", "/w", Timestamp(1))
        .unwrap_err();
    assert!(error.to_string().contains("pid 1"), "{error}");

    let error = h
        .manager
        .shell_command_started(SessionId(999), SHELL_PID, 1, "ls", "/w", Timestamp(1))
        .unwrap_err();
    assert!(matches!(error, SessionManagerError::UnknownSession(_)));
}

#[test]
fn pty_operations_on_a_commands_session_are_denied() {
    let h = harness();
    let session = register(&h);
    for error in [
        h.manager.attach(session.id).map(|_| ()).unwrap_err(),
        h.manager.resize(session.id, 100, 30).unwrap_err(),
        h.manager
            .write_input(session.id, b"ls\r", "100.64.0.1:1")
            .unwrap_err(),
        h.manager.kill(session.id).unwrap_err(),
    ] {
        assert!(error.to_string().contains("COMMANDS"), "{error}");
    }
    assert_eq!(
        h.manager.session(session.id).unwrap().state,
        SessionState::Idle
    );
}

// ---- lifecycle -------------------------------------------------------------

#[test]
fn a_live_shell_stays_registered() {
    let h = harness();
    let session = register(&h);
    assert!(h.manager.check_external_processes().unwrap().is_empty());
    assert_eq!(
        h.manager.session(session.id).unwrap().state,
        SessionState::Idle
    );
}

#[test]
fn an_exited_shell_ends_the_session_with_unknown_exit_code() {
    let h = harness();
    let session = register(&h);
    start(&h, session.id, 1, "sleep 100", 1_790_000_001_000);
    h.processes.lock().unwrap().remove(&SHELL_PID);

    assert_eq!(
        h.manager.check_external_processes().unwrap(),
        vec![session.id]
    );
    let ended = h.manager.session(session.id).unwrap();
    assert_eq!(ended.state, SessionState::Completed);
    assert_eq!(ended.exit_code, None);
    assert_eq!(
        h.store.session(session.id).unwrap().unwrap().state,
        SessionState::Completed
    );
    let events = h.store.events_since(protocol::EventId(0)).unwrap();
    assert!(events.iter().any(|event| matches!(
        event.kind,
        EventKind::ProcessExited {
            pid: SHELL_PID,
            exit_code: None
        }
    )));
    assert!(h.manager.process_roots().unwrap().is_empty());
    // Ending is reported once.
    assert!(h.manager.check_external_processes().unwrap().is_empty());
}

#[test]
fn a_reused_pid_is_not_mistaken_for_the_original_shell() {
    let h = harness();
    let session = register(&h);
    h.processes.lock().unwrap().insert(
        SHELL_PID,
        ProcessIdentity {
            started_at: Timestamp(SHELL_STARTED.0 + 1_000),
            uid: OWNER,
        },
    );
    assert_eq!(
        h.manager.check_external_processes().unwrap(),
        vec![session.id]
    );
    assert_eq!(
        h.manager.session(session.id).unwrap().state,
        SessionState::Completed
    );
}

#[test]
fn restart_recovers_a_verified_live_shell_and_ends_an_unverifiable_one() {
    let h = harness();
    let alive = register(&h);
    h.processes.lock().unwrap().insert(
        5_000,
        ProcessIdentity {
            started_at: SHELL_STARTED,
            uid: OWNER,
        },
    );
    let gone = h
        .manager
        .register_shell(5_000, "/bin/bash", "/tmp", None)
        .unwrap();
    start(&h, alive.id, 1, "make", 1_790_000_001_000);
    h.processes.lock().unwrap().remove(&5_000);
    let Harness {
        store,
        data_dir,
        processes,
        manager,
    } = h;
    drop(manager);

    // A restarted daemon: FULL Sessions are interrupted, external ones checked.
    store
        .interrupt_orphaned_sessions(Timestamp(1_790_000_100_000))
        .unwrap();
    let live = store.live_sessions().unwrap();
    let h = harness_with(store, data_dir, processes);
    h.manager.recover_external_sessions(live).unwrap();

    let recovered = h.manager.session(alive.id).unwrap();
    assert_eq!(recovered.origin, SessionOrigin::ShellHook);
    assert_eq!(recovered.state, SessionState::Idle);
    assert!(matches!(
        h.manager.session(gone.id),
        Err(SessionManagerError::UnknownSession(_))
    ));
    assert_eq!(
        h.store.session(gone.id).unwrap().unwrap().state,
        SessionState::Completed
    );
    // The recovered shell keeps reporting into the same Session.
    assert_eq!(register(&h).id, alive.id);
    start(&h, alive.id, 2, "ls", 1_790_000_200_000);
    assert!(finish(&h, alive.id, 2, 0, 1_790_000_200_010).is_some());
}

// ---- local socket dispatch and privacy --------------------------------------

async fn local(h: &Harness, request: LocalRequest) -> LocalResponse {
    let (mut client, server) = tokio::net::UnixStream::pair().unwrap();
    let manager = h.manager.clone();
    let audit = SecurityAudit::new(&h.data_dir);
    let path = h.data_dir.join("run/sentinald.sock");
    let task = tokio::spawn(async move {
        handle_connection(server, OWNER, Some(OWNER), audit, &path, &move |request| {
            manager.handle_local(request)
        })
        .await
    });
    let body = Envelope::new(request).encode().unwrap();
    client
        .write_all(&u32::try_from(body.len()).unwrap().to_be_bytes())
        .await
        .unwrap();
    client.write_all(body.as_bytes()).await.unwrap();
    let mut length = [0_u8; 4];
    client.read_exact(&mut length).await.unwrap();
    let mut reply = vec![0; u32::from_be_bytes(length) as usize];
    client.read_exact(&mut reply).await.unwrap();
    task.await.unwrap().unwrap();
    Envelope::<LocalResponse>::decode(std::str::from_utf8(&reply).unwrap())
        .unwrap()
        .payload
}

#[tokio::test]
async fn local_socket_requests_drive_the_session_and_store_no_output_or_input() {
    let h = harness();
    let LocalResponse::Session { session } = local(
        &h,
        LocalRequest::RegisterShell {
            pid: SHELL_PID,
            shell: "/bin/bash".into(),
            cwd: "/work".into(),
            title: None,
        },
    )
    .await
    else {
        panic!("expected a Session");
    };
    let started = local(
        &h,
        LocalRequest::ShellCommandStarted {
            session_id: session.id,
            pid: SHELL_PID,
            sequence: 1,
            text: "cat secrets.txt".into(),
            cwd: "/work".into(),
            started_at: Timestamp(1_790_000_001_000),
        },
    )
    .await;
    assert!(matches!(started, LocalResponse::Command { .. }));
    let finished = local(
        &h,
        LocalRequest::ShellCommandFinished {
            session_id: session.id,
            pid: SHELL_PID,
            sequence: 1,
            exit_code: 0,
            finished_at: Timestamp(1_790_000_001_020),
        },
    )
    .await;
    assert!(matches!(
        finished,
        LocalResponse::Command { command } if command.exit_code == Some(0)
    ));

    let forbidden = local(
        &h,
        LocalRequest::RegisterShell {
            pid: 1,
            shell: "/bin/sh".into(),
            cwd: "/".into(),
            title: None,
        },
    )
    .await;
    assert!(matches!(
        forbidden,
        LocalResponse::Error {
            code: LocalErrorCode::NotFound,
            ..
        }
    ));
    let adopt = local(
        &h,
        LocalRequest::AdoptProcess {
            pid: SHELL_PID,
            title: None,
            log_path: None,
        },
    )
    .await;
    assert!(matches!(
        adopt,
        LocalResponse::Error {
            code: LocalErrorCode::Unavailable,
            ..
        }
    ));

    // COMMANDS capture keeps no terminal log, no input audit, no screen.
    assert!(!h
        .data_dir
        .join(format!("sessions/{}.log.zst", session.id.0))
        .exists());
    assert!(!h.data_dir.join("audit.jsonl").exists());
    assert!(h.manager.scrollback(session.id).unwrap().is_empty());
    fs::remove_dir_all(&h.data_dir).unwrap();
}
