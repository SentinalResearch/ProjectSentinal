//! Adopted log tail lifecycle: activation, watcher integration, fallback,
//! final drain, descendant liveness, and restart cursor persistence.
//!
//! Background polling is disabled; each test drives the tail with
//! `poll_log_tail`, so nothing sleeps.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    process_probe::{ProcessDetails, ProcessIdentity, ProcessProbe},
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{
    CaptureLevel, EventKind, HealthOverall, LogSource, MachineId, Process, ProcessState, Session,
    SessionId, SessionLog, SessionState, Timestamp,
};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use store::Store;

const TARGET: u32 = 7_001;
const CHILD: u32 = 7_002;
const STARTED: Timestamp = Timestamp(1_789_000_000_000);

static NEXT: AtomicU64 = AtomicU64::new(1);

type Table = Arc<Mutex<HashMap<u32, ProcessIdentity>>>;

struct Harness {
    manager: SessionManager,
    table: Table,
    store: Arc<Store>,
    clock: Arc<AtomicI64>,
    data_dir: PathBuf,
    log: PathBuf,
    argv: Option<Vec<String>>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.data_dir);
    }
}

fn owner() -> u32 {
    fs::metadata(std::env::current_exe().unwrap())
        .unwrap()
        .uid()
}

fn identity() -> ProcessIdentity {
    ProcessIdentity {
        started_at: STARTED,
        uid: owner(),
    }
}

fn manager(
    store: Arc<Store>,
    table: Table,
    clock: Arc<AtomicI64>,
    data_dir: &Path,
    argv: Option<Vec<String>>,
    discovered: Option<SessionLog>,
) -> SessionManager {
    let lookup = table.clone();
    let probe = ProcessProbe::new(owner(), move |pid| {
        lookup.lock().unwrap().get(&pid).copied()
    })
    .with_details(move |_| ProcessDetails {
        name: Some("python3".into()),
        exe: Some("/usr/bin/python3".into()),
        cwd: Some("/work".into()),
        command: argv.clone(),
    })
    .with_log_discovery(move |_| discovered.clone());
    SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.to_path_buf(),
            scrollback_lines: 10,
            clock: Arc::new(move || Timestamp(clock.load(Ordering::SeqCst))),
        },
        AuditLog::new(data_dir),
        store,
    )
    .unwrap()
    .with_process_probe(probe)
    .with_log_tail_polling(false)
}

fn harness_with(argv: &[&str], discovered: Option<SessionLog>) -> Harness {
    let data_dir = std::env::temp_dir().join(format!(
        "sentinal-adopted-log-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&data_dir).unwrap();
    let table: Table = Arc::new(Mutex::new(HashMap::from([(TARGET, identity())])));
    let store = Arc::new(Store::open(&data_dir.join("s.sqlite3")).unwrap());
    let clock = Arc::new(AtomicI64::new(1_790_000_000_000));
    let argv = (!argv.is_empty()).then(|| argv.iter().map(|value| (*value).to_string()).collect());
    let manager = manager(
        store.clone(),
        table.clone(),
        clock.clone(),
        &data_dir,
        argv.clone(),
        discovered,
    );
    Harness {
        manager,
        table,
        store,
        clock,
        log: data_dir.join("train.log"),
        data_dir,
        argv,
    }
}

fn harness() -> Harness {
    harness_with(&["python3", "train.py", "--epochs", "10"], None)
}

impl Harness {
    fn adopt(&self, contents: &str) -> Session {
        fs::write(&self.log, contents).unwrap();
        self.manager
            .adopt_process(
                TARGET,
                Some("Run".into()),
                Some(self.log.display().to_string()),
            )
            .unwrap()
    }

    fn append(&self, contents: &[u8]) {
        fs::OpenOptions::new()
            .append(true)
            .open(&self.log)
            .unwrap()
            .write_all(contents)
            .unwrap();
    }

    fn events(&self, id: SessionId) -> Vec<EventKind> {
        self.store
            .events(Some(id), None, 500)
            .unwrap()
            .into_iter()
            .map(|event| event.kind)
            .collect()
    }

    fn progress(&self, id: SessionId, current: u64) -> usize {
        self.events(id)
            .iter()
            .filter(|kind| matches!(kind, EventKind::ProgressUpdated { current: c, .. } if *c == current))
            .count()
    }

    fn warnings(&self, id: SessionId) -> Vec<String> {
        self.events(id)
            .into_iter()
            .filter_map(|kind| match kind {
                EventKind::WarningDetected { message } => Some(message),
                _ => None,
            })
            .collect()
    }

    fn restart(&self) -> SessionManager {
        self.clock.fetch_add(60_000, Ordering::SeqCst);
        let restarted = manager(
            self.store.clone(),
            self.table.clone(),
            self.clock.clone(),
            &self.data_dir,
            self.argv.clone(),
            None,
        );
        restarted
            .recover_external_sessions(self.store.live_sessions().unwrap())
            .unwrap();
        restarted
    }
}

fn process(pid: u32, parent: Option<u32>) -> Process {
    Process {
        pid,
        parent_pid: parent,
        name: "python3".into(),
        cmdline: Vec::new(),
        state: ProcessState::Running,
        cpu_percent: 1.0,
        rss_bytes: 1,
        gpu_mem_bytes: None,
        started_at: STARTED,
    }
}

// ---- activation and watcher integration -------------------------------------

#[tokio::test]
async fn adopted_argv_selects_the_same_adapter_as_a_full_session() {
    let h = harness();
    let session = h.adopt("Epoch 1/10 loss=0.9\n");

    assert_eq!(session.capture, CaptureLevel::ProcessLog);
    assert!(h.manager.poll_log_tail(session.id).unwrap());
    assert_eq!(h.progress(session.id, 1), 1);
}

#[tokio::test]
async fn log_lines_never_become_commands() {
    let h = harness_with(&["python3", "serve.py"], None);
    let session = h.adopt("$ rm -rf /tmp/x\nCommand: deploy --prod\n");

    h.manager.poll_log_tail(session.id).unwrap();

    assert!(h.store.history(Some(session.id), 10).unwrap().is_empty());
    assert!(!h.events(session.id).iter().any(|kind| matches!(
        kind,
        EventKind::CommandStarted { .. } | EventKind::CommandFinished { .. }
    )));
}

#[tokio::test]
async fn error_lines_in_a_log_warn_but_never_mark_the_session_failed() {
    let h = harness_with(&["python3", "job.py"], None);
    let session = h.adopt("Traceback (most recent call last):\n");

    h.manager.poll_log_tail(session.id).unwrap();

    assert!(h
        .events(session.id)
        .iter()
        .any(|kind| matches!(kind, EventKind::ErrorDetected { .. })));
    let health = h.manager.session(session.id).unwrap().health.unwrap();
    assert_eq!(health.overall, HealthOverall::Warning);
}

// ---- fallback ---------------------------------------------------------------

#[tokio::test]
async fn a_path_swapped_for_a_symlink_falls_back_to_process_with_a_warning() {
    let h = harness();
    let session = h.adopt("Epoch 1/10\n");
    h.manager.poll_log_tail(session.id).unwrap();
    let target = h.data_dir.join("secret.txt");
    fs::write(&target, "Epoch 9/10\n").unwrap();
    fs::remove_file(&h.log).unwrap();
    std::os::unix::fs::symlink(&target, &h.log).unwrap();

    assert!(!h.manager.poll_log_tail(session.id).unwrap());

    assert_eq!(
        h.manager.session(session.id).unwrap().capture,
        CaptureLevel::Process
    );
    assert_eq!(
        h.store.session(session.id).unwrap().unwrap().capture,
        CaptureLevel::Process
    );
    assert!(h.warnings(session.id)[0].starts_with("Sentinal stopped reading the log"));
    assert_eq!(
        h.progress(session.id, 9),
        0,
        "the symlink target was not read"
    );
    assert!(
        !h.manager.poll_log_tail(session.id).unwrap(),
        "the tail stays stopped"
    );
}

#[tokio::test]
async fn a_backlog_over_the_limit_is_skipped_with_a_structured_warning() {
    let h = harness();
    let session = h.adopt("start\n");
    h.manager.poll_log_tail(session.id).unwrap();
    h.append(&vec![b'x'; 9 * 1024 * 1024]);
    h.append(b"\nEpoch 4/10\n");

    h.manager.poll_log_tail(session.id).unwrap();

    assert!(h.warnings(session.id)[0].starts_with("Sentinal skipped"));
    assert_eq!(
        h.manager.session(session.id).unwrap().capture,
        CaptureLevel::ProcessLog
    );
}

#[tokio::test]
async fn an_unreadable_discovered_log_warns_and_stays_process() {
    let missing = SessionLog {
        path: "/nonexistent/sentinal/train.log".into(),
        source: LogSource::Stdout,
    };
    let h = harness_with(&["python3", "train.py"], Some(missing));
    let session = h.manager.adopt_process(TARGET, None, None).unwrap();

    tokio::time::timeout(Duration::from_millis(100), async {
        while h.warnings(session.id).is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();

    let after = h.manager.session(session.id).unwrap();
    assert_eq!(after.capture, CaptureLevel::Process);
    assert!(after.log.is_some());
    assert!(h.warnings(session.id)[0].starts_with("Sentinal cannot read the log"));
}

// ---- process end ------------------------------------------------------------

#[tokio::test]
async fn process_end_drains_final_bytes_and_never_fabricates_an_exit_code() {
    let h = harness();
    let session = h.adopt("Epoch 1/10\n");
    h.manager.poll_log_tail(session.id).unwrap();
    h.append(b"Epoch 2/10\nEpoch 3/10");
    h.table.lock().unwrap().remove(&TARGET);

    assert_eq!(
        h.manager.check_external_processes().unwrap(),
        vec![session.id]
    );

    assert_eq!(h.progress(session.id, 2), 1);
    assert_eq!(
        h.progress(session.id, 3),
        1,
        "the unterminated final line is drained"
    );
    let ended = h.manager.session(session.id).unwrap();
    assert_eq!(
        (ended.state, ended.exit_code),
        (SessionState::Completed, None)
    );
    assert!(h.events(session.id).iter().any(|kind| matches!(
        kind,
        EventKind::ProcessExited {
            exit_code: None,
            ..
        }
    )));
    assert!(!h.manager.poll_log_tail(session.id).unwrap());
    assert_eq!(h.store.log_cursor(session.id).unwrap(), None);
}

#[tokio::test]
async fn a_surviving_descendant_keeps_the_adopted_session_and_tail_alive() {
    let h = harness();
    let session = h.adopt("Epoch 1/10\n");
    h.table.lock().unwrap().insert(CHILD, identity());
    h.manager
        .publish_processes(
            session.id,
            Timestamp(1),
            vec![process(TARGET, None), process(CHILD, Some(TARGET))],
        )
        .unwrap();

    h.table.lock().unwrap().remove(&TARGET);
    assert!(h.manager.check_external_processes().unwrap().is_empty());
    assert!(h
        .manager
        .process_roots()
        .unwrap()
        .contains(&(session.id, CHILD)));
    assert!(h.manager.poll_log_tail(session.id).unwrap());

    h.table.lock().unwrap().remove(&CHILD);
    assert_eq!(
        h.manager.check_external_processes().unwrap(),
        vec![session.id]
    );
}

// ---- restart ----------------------------------------------------------------

#[tokio::test]
async fn restart_resumes_from_the_cursor_without_replaying_the_log() {
    let h = harness();
    let session = h.adopt("Epoch 1/10\n");
    h.manager.poll_log_tail(session.id).unwrap();
    assert!(h.store.log_cursor(session.id).unwrap().is_some());
    h.append(b"Epoch 2/10\n");

    let restarted = h.restart();
    assert!(restarted.poll_log_tail(session.id).unwrap());

    assert_eq!(h.progress(session.id, 1), 1, "no replay after restart");
    assert_eq!(h.progress(session.id, 2), 1);
    assert_eq!(
        restarted.session(session.id).unwrap().capture,
        CaptureLevel::ProcessLog
    );
}

#[tokio::test]
async fn restart_after_the_log_was_replaced_reads_the_new_file_safely() {
    let h = harness();
    let session = h.adopt("Epoch 1/10\n");
    h.manager.poll_log_tail(session.id).unwrap();
    let replacement = h.data_dir.join("new.log");
    fs::write(&replacement, "Epoch 5/10\n").unwrap();
    fs::rename(&replacement, &h.log).unwrap();

    let restarted = h.restart();
    restarted.poll_log_tail(session.id).unwrap();

    assert_eq!(h.progress(session.id, 5), 1);
    assert_eq!(h.progress(session.id, 1), 1);
}

#[tokio::test]
async fn restart_with_a_missing_log_falls_back_to_process_with_a_warning() {
    let h = harness();
    let session = h.adopt("Epoch 1/10\n");
    h.manager.poll_log_tail(session.id).unwrap();
    fs::remove_file(&h.log).unwrap();

    let restarted = h.restart();

    let recovered = restarted.session(session.id).unwrap();
    assert_eq!(recovered.state, SessionState::Running);
    assert_eq!(recovered.capture, CaptureLevel::Process);
    assert!(h.warnings(session.id)[0].starts_with("Sentinal cannot read the log"));
}
