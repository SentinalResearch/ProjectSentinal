//! Read-only adoption of existing same-user processes (ADOPTED / PROCESS).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    api::{AppState, WsAction},
    audit::AuditLog,
    auth::AuthManager,
    process_probe::{ProcessDetails, ProcessIdentity, ProcessProbe},
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig, SessionManagerError},
};
use protocol::{
    CaptureLevel, ClientRequest, DaemonMessage, Envelope, EventKind, LocalErrorCode, LocalRequest,
    LocalResponse, LogSource, Machine, MachineId, MachineStatus, Os, SessionId, SessionLog,
    SessionOrigin, SessionState, Timestamp,
};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use store::Store;

const TARGET: u32 = 7_001;
const STARTED: Timestamp = Timestamp(1_789_000_000_000);

static NEXT: AtomicU64 = AtomicU64::new(1);

/// The test runner's UID, which owns the temp log files these tests create.
fn owner() -> u32 {
    fs::metadata(std::env::current_exe().unwrap())
        .unwrap()
        .uid()
}

type Table = Arc<Mutex<HashMap<u32, ProcessIdentity>>>;

/// Counts every probe call; the probe type offers nothing but reads.
#[derive(Default)]
struct Spy {
    identify: AtomicUsize,
    details: AtomicUsize,
    discover: AtomicUsize,
}

struct Harness {
    manager: SessionManager,
    table: Table,
    spy: Arc<Spy>,
    store: Arc<Store>,
    data_dir: PathBuf,
}

fn harness_with(discovered: Option<SessionLog>, details: ProcessDetails) -> Harness {
    let data_dir = std::env::temp_dir().join(format!(
        "sentinal-adopt-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&data_dir).unwrap();
    let table: Table = Arc::new(Mutex::new(HashMap::from([(
        TARGET,
        ProcessIdentity {
            started_at: STARTED,
            uid: owner(),
        },
    )])));
    let spy = Arc::new(Spy::default());
    let (lookup, spy_identify, spy_details, spy_discover) =
        (table.clone(), spy.clone(), spy.clone(), spy.clone());
    let probe = ProcessProbe::new(owner(), move |pid| {
        spy_identify.identify.fetch_add(1, Ordering::SeqCst);
        lookup.lock().unwrap().get(&pid).copied()
    })
    .with_details(move |_| {
        spy_details.details.fetch_add(1, Ordering::SeqCst);
        details.clone()
    })
    .with_log_discovery(move |_| {
        spy_discover.discover.fetch_add(1, Ordering::SeqCst);
        discovered.clone()
    });
    let store = Arc::new(Store::open(&data_dir.join("s.sqlite3")).unwrap());
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
    .with_process_probe(probe);
    Harness {
        manager,
        table,
        spy,
        store,
        data_dir,
    }
}

fn python_details() -> ProcessDetails {
    ProcessDetails {
        name: Some("python3".into()),
        exe: Some("/usr/bin/python3".into()),
        cwd: Some("/work/TIM".into()),
        command: None,
    }
}

fn harness() -> Harness {
    harness_with(None, python_details())
}

async fn eventually(mut ready: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_millis(100), async {
        while !ready() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

// ---- resolution ------------------------------------------------------------

#[tokio::test]
async fn a_same_user_process_is_adopted_read_only_with_metadata() {
    let h = harness();
    let session = h
        .manager
        .adopt_process(TARGET, Some("Qwen Training".into()), None)
        .unwrap();

    assert_eq!(session.origin, SessionOrigin::Adopted);
    assert_eq!(session.capture, CaptureLevel::Process);
    assert_eq!(session.pid, Some(TARGET));
    assert_eq!(session.process_started_at, Some(STARTED));
    assert_eq!(session.state, SessionState::Running);
    assert_eq!(session.title.as_deref(), Some("Qwen Training"));
    assert_eq!(session.shell, "/usr/bin/python3");
    assert_eq!(session.cwd, "/work/TIM");
    assert_eq!(session.exit_code, None);
    assert_eq!(h.store.session(session.id).unwrap().unwrap(), session);
    assert_eq!(
        h.manager.process_roots().unwrap(),
        vec![(session.id, TARGET)]
    );
}

#[tokio::test]
async fn missing_metadata_is_not_an_error() {
    let h = harness_with(None, ProcessDetails::default());
    let session = h.manager.adopt_process(TARGET, None, None).unwrap();
    assert_eq!((session.shell.as_str(), session.cwd.as_str()), ("", ""));
    assert_eq!(session.title, None);
}

#[tokio::test]
async fn the_process_name_is_the_default_title() {
    let h = harness();
    let session = h.manager.adopt_process(TARGET, None, None).unwrap();
    assert_eq!(session.title.as_deref(), Some("python3"));
}

#[tokio::test]
async fn invalid_targets_are_rejected_with_named_errors_and_create_nothing() {
    let h = harness();
    h.table.lock().unwrap().insert(
        8_000,
        ProcessIdentity {
            started_at: STARTED,
            uid: 0,
        },
    );
    let cases: Vec<(u32, &str)> = vec![
        (8_000, "uid 0"),
        (9_999, "no process with pid 9999"),
        (std::process::id(), "sentinald itself"),
    ];
    for (pid, expected) in cases {
        if pid == std::process::id() {
            h.table.lock().unwrap().insert(
                pid,
                ProcessIdentity {
                    started_at: STARTED,
                    uid: owner(),
                },
            );
        }
        let error = h.manager.adopt_process(pid, None, None).unwrap_err();
        assert!(error.to_string().contains(expected), "{pid}: {error}");
    }
    let zero = h.manager.handle_local(LocalRequest::AdoptProcess {
        pid: 0,
        title: None,
        log_path: None,
    });
    assert!(matches!(
        zero,
        LocalResponse::Error {
            code: LocalErrorCode::InvalidRequest,
            ..
        }
    ));
    assert!(h.manager.list().unwrap().is_empty());
}

#[tokio::test]
async fn adopting_the_same_process_twice_is_rejected() {
    let h = harness();
    let first = h.manager.adopt_process(TARGET, None, None).unwrap();
    let error = h.manager.adopt_process(TARGET, None, None).unwrap_err();
    assert!(
        matches!(error, SessionManagerError::AlreadyAdopted { session_id, .. } if session_id == first.id),
        "{error}"
    );
    let reply = h.manager.handle_local(LocalRequest::AdoptProcess {
        pid: TARGET,
        title: None,
        log_path: None,
    });
    assert!(matches!(
        reply,
        LocalResponse::Error {
            code: LocalErrorCode::Conflict,
            ..
        }
    ));
}

#[tokio::test]
async fn a_reused_pid_never_inherits_the_old_session() {
    let h = harness();
    let old = h.manager.adopt_process(TARGET, None, None).unwrap();
    h.table.lock().unwrap().insert(
        TARGET,
        ProcessIdentity {
            started_at: Timestamp(STARTED.0 + 5_000),
            uid: owner(),
        },
    );
    let new = h.manager.adopt_process(TARGET, None, None).unwrap();
    assert_ne!(old.id, new.id);
    let old = h.manager.session(old.id).unwrap();
    assert_eq!(old.state, SessionState::Completed);
    assert_eq!(old.exit_code, None);
}

// ---- read-only -------------------------------------------------------------

fn app(h: &Harness) -> AppState {
    AppState::new(
        h.manager.clone(),
        Machine {
            id: MachineId("test-machine".into()),
            name: "t".into(),
            os: Os::Linux,
            cpu: "c".into(),
            cpu_cores: 1,
            ram_bytes: 1,
            gpus: vec![],
            status: MachineStatus::Online,
            last_seen: Timestamp(1),
        },
        AuthManager::load(&h.data_dir.join("auth.json"), "t".into()).unwrap(),
    )
}

#[tokio::test]
async fn handcrafted_remote_control_requests_all_fail_and_only_reads_happen() {
    let h = harness();
    let session = h.manager.adopt_process(TARGET, None, None).unwrap();
    let app = app(&h);
    for request in [
        ClientRequest::Attach {
            session_id: session.id,
        },
        ClientRequest::Resize {
            session_id: session.id,
            cols: 10,
            rows: 10,
        },
        ClientRequest::Kill {
            session_id: session.id,
        },
    ] {
        let action = app
            .dispatch_text(&Envelope::new(request).encode().unwrap())
            .await;
        assert!(
            matches!(action, WsAction::Reply(DaemonMessage::Error { ref message }) if message.contains("PROCESS")),
        );
    }
    let input = h
        .manager
        .write_input(session.id, b"\x03kill -9 7001\r", "100.64.0.9:1")
        .unwrap_err();
    assert!(input.to_string().contains("PROCESS"), "{input}");

    assert_eq!(
        h.manager.session(session.id).unwrap().state,
        SessionState::Running
    );
    assert!(
        h.table.lock().unwrap().contains_key(&TARGET),
        "process untouched"
    );
    assert!(!h.data_dir.join("audit.jsonl").exists());
    // Adoption used only the probe's read methods.
    assert!(h.spy.identify.load(Ordering::SeqCst) >= 1);
    assert_eq!(h.spy.details.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn handcrafted_control_requests_fail_for_process_log_sessions_too() {
    let h = harness();
    let log = h.data_dir.join("job.log");
    fs::write(&log, "started\n").unwrap();
    let session = h
        .manager
        .adopt_process(TARGET, None, Some(log.display().to_string()))
        .unwrap();
    assert_eq!(session.capture, CaptureLevel::ProcessLog);
    let app = app(&h);
    for request in [
        ClientRequest::Attach {
            session_id: session.id,
        },
        ClientRequest::Resize {
            session_id: session.id,
            cols: 10,
            rows: 10,
        },
        ClientRequest::Kill {
            session_id: session.id,
        },
    ] {
        let action = app
            .dispatch_text(&Envelope::new(request.clone()).encode().unwrap())
            .await;
        assert!(
            matches!(action, WsAction::Reply(DaemonMessage::Error { ref message }) if message.contains("PROCESS_LOG")),
            "{request:?} was not rejected"
        );
    }
    let input = h
        .manager
        .write_input(session.id, b"y\r", "100.64.0.9:1")
        .unwrap_err();
    assert!(input.to_string().contains("PROCESS_LOG"), "{input}");
    assert!(
        !h.data_dir.join("audit.jsonl").exists(),
        "no input was audited"
    );
    assert_eq!(
        fs::read(&log).unwrap(),
        b"started\n",
        "the log is untouched"
    );
}

// ---- exit ------------------------------------------------------------------

#[tokio::test]
async fn an_exited_process_ends_with_unknown_exit_and_no_fabricated_command() {
    let h = harness();
    let session = h.manager.adopt_process(TARGET, None, None).unwrap();
    h.table.lock().unwrap().remove(&TARGET);

    assert_eq!(
        h.manager.check_external_processes().unwrap(),
        vec![session.id]
    );
    let ended = h.manager.session(session.id).unwrap();
    assert_eq!(ended.state, SessionState::Completed);
    assert_eq!(ended.exit_code, None);
    assert!(h.store.history(Some(session.id), 10).unwrap().is_empty());
    let events = h.store.events_since(protocol::EventId(0)).unwrap();
    assert!(events.iter().any(|event| matches!(
        event.kind,
        EventKind::ProcessExited {
            pid: TARGET,
            exit_code: None
        }
    )));
    assert!(!events
        .iter()
        .any(|event| matches!(event.kind, EventKind::CommandFinished { .. })));
}

// ---- logs ------------------------------------------------------------------

#[tokio::test]
async fn an_explicit_log_upgrades_capture_and_emits_training_progress() {
    let h = harness();
    let log = h.data_dir.join("train.log");
    fs::write(&log, "Epoch 1/10 loss=0.9\n").unwrap();
    let path = log.display().to_string();
    let session = h
        .manager
        .adopt_process(TARGET, Some("Training".into()), Some(path.clone()))
        .unwrap();
    assert_eq!(
        session.log,
        Some(SessionLog {
            path,
            source: LogSource::Explicit
        })
    );
    assert_eq!(session.capture, CaptureLevel::ProcessLog);
    eventually(|| {
        h.store
            .events(Some(session.id), None, 20)
            .unwrap()
            .iter()
            .any(|event| {
                matches!(
                    event.kind,
                    EventKind::ProgressUpdated {
                        current: 1,
                        total: Some(10),
                        ..
                    }
                )
            })
    })
    .await;
    // An explicit log skips discovery.
    tokio::task::yield_now().await;
    assert_eq!(h.spy.discover.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unusable_explicit_logs_are_rejected_before_adoption() {
    let h = harness();
    let cases = [
        (h.data_dir.display().to_string(), "not a regular file"),
        (
            h.data_dir.join("absent.log").display().to_string(),
            "cannot read",
        ),
        ("relative.log".to_string(), "absolute"),
    ];
    for (path, expected) in cases {
        let error = h
            .manager
            .adopt_process(TARGET, None, Some(path.clone()))
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{path}: {error}");
    }
    assert!(h.manager.list().unwrap().is_empty());
}

#[tokio::test]
async fn an_explicit_log_owned_by_another_uid_is_rejected() {
    let seed = harness();
    let log = seed.data_dir.join("foreign.log");
    fs::write(&log, "private output\n").unwrap();
    let file_uid = fs::metadata(&log).unwrap().uid();

    let table: Table = Arc::new(Mutex::new(HashMap::from([(
        TARGET,
        ProcessIdentity {
            started_at: STARTED,
            uid: file_uid.wrapping_add(1),
        },
    )])));
    let lookup = table.clone();
    let manager = SessionManager::new(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: seed.data_dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(1_790_000_000_000)),
        },
        AuditLog::new(&seed.data_dir),
    )
    .with_process_probe(ProcessProbe::new(file_uid.wrapping_add(1), move |pid| {
        lookup.lock().unwrap().get(&pid).copied()
    }));

    let error = manager
        .adopt_process(TARGET, None, Some(log.display().to_string()))
        .unwrap_err();
    assert!(error.to_string().contains("log uid"), "{error}");
    assert!(manager.list().unwrap().is_empty());
}

#[tokio::test]
async fn a_discovered_log_is_recorded_off_the_request_path() {
    let found = SessionLog {
        path: "/work/TIM/train.log".into(),
        source: LogSource::StdoutAndStderr,
    };
    let h = harness_with(Some(found.clone()), python_details());
    let session = h.manager.adopt_process(TARGET, None, None).unwrap();
    assert_eq!(session.log, None, "the reply does not wait for discovery");

    eventually(|| h.manager.session(session.id).unwrap().log.is_some()).await;
    let updated = h.manager.session(session.id).unwrap();
    assert_eq!(updated.log, Some(found));
    assert_eq!(updated.capture, CaptureLevel::Process);
    eventually(|| h.store.session(session.id).unwrap().unwrap().log.is_some()).await;
}

#[tokio::test]
async fn failed_discovery_leaves_process_capture_without_a_log() {
    let h = harness();
    let session = h.manager.adopt_process(TARGET, None, None).unwrap();
    eventually(|| h.spy.discover.load(Ordering::SeqCst) == 1).await;
    let after = h.manager.session(session.id).unwrap();
    assert_eq!((after.capture, after.log), (CaptureLevel::Process, None));
}

// ---- restart ---------------------------------------------------------------

#[tokio::test]
async fn restart_keeps_a_verified_adopted_process_running() {
    let h = harness();
    let session = h.manager.adopt_process(TARGET, None, None).unwrap();
    let live = h.store.live_sessions().unwrap();
    let restarted = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: h.data_dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(1_790_000_100_000)),
        },
        AuditLog::new(&h.data_dir),
        h.store.clone(),
    )
    .unwrap()
    .with_process_probe({
        let table = h.table.clone();
        ProcessProbe::new(owner(), move |pid| table.lock().unwrap().get(&pid).copied())
    });
    restarted.recover_external_sessions(live).unwrap();
    let recovered = restarted.session(session.id).unwrap();
    assert_eq!(recovered.state, SessionState::Running);
    assert_eq!(recovered.origin, SessionOrigin::Adopted);
    let _ = SessionId(0);
}

// ---- live observation without attaching --------------------------------------

#[tokio::test]
async fn reduced_capture_metrics_and_endings_reach_unattached_observers() {
    let h = harness();
    let mut observer = h.manager.observe();
    let session = h.manager.adopt_process(TARGET, None, None).unwrap();

    h.manager
        .publish_metrics(protocol::SessionMetrics {
            session_id: session.id,
            at: Timestamp(2),
            totals: protocol::ProcessTotals {
                cpu_percent: 42.0,
                rss_bytes: 1024,
                gpu_mem_bytes: 0,
            },
        })
        .unwrap();
    h.table.lock().unwrap().remove(&TARGET);
    h.manager.check_external_processes().unwrap();

    let mut saw_metrics = false;
    let mut saw_end = false;
    while let Ok(message) = observer.try_recv() {
        match message {
            DaemonMessage::SessionMetrics { metrics } if metrics.session_id == session.id => {
                saw_metrics = metrics.totals.cpu_percent == 42.0;
            }
            DaemonMessage::SessionUpdated { session: updated } if updated.id == session.id => {
                saw_end |= updated.state == SessionState::Completed && updated.exit_code.is_none();
            }
            _ => {}
        }
    }
    assert!(saw_metrics && saw_end);
}
