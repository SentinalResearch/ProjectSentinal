//! Milestones 6.4 and 6.5: an agent inside a Session reports what it is doing,
//! and how far along it is, over the private local socket. Only a process
//! inside that Session may.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    local_socket::{handle_connection, sync_handler, SecurityAudit},
    process_probe::ProcessProbe,
    pty::FakePty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{
    AgentStatus, Envelope, LocalErrorCode, LocalRequest, LocalResponse, MachineId, SessionId,
    SessionOrigin, SessionState, Timestamp,
};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc,
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const OWNER: u32 = 501;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

struct Harness {
    manager: SessionManager,
    backend: Arc<FakePty>,
    clock: Arc<AtomicI64>,
    data_dir: PathBuf,
}

impl Drop for Harness {
    /// End every live fake PTY, or its reader keeps the test runtime alive.
    fn drop(&mut self) {
        self.manager.shutdown();
    }
}

/// A manager whose process tree puts this test process inside Session 1's
/// fake root (pid 10_001), so a report sent over a real socketpair (the OS
/// names this process as the peer) counts as coming from inside it.
fn harness(inside: bool) -> Harness {
    let data_dir = std::env::temp_dir().join(format!(
        "sentinal-agent-state-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&data_dir).unwrap();
    let backend = Arc::new(FakePty::new());
    let clock = Arc::new(AtomicI64::new(1_790_000_000_000));
    let now = clock.clone();
    let me = std::process::id();
    let parents: HashMap<u32, u32> = if inside {
        HashMap::from([(me, 20_000), (20_000, 10_001)])
    } else {
        HashMap::from([(me, 1)])
    };
    let manager = SessionManager::new(
        backend.clone(),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 3,
            clock: Arc::new(move || Timestamp(now.load(Ordering::SeqCst))),
        },
        AuditLog::new(&data_dir),
    )
    .with_process_probe(
        ProcessProbe::new(OWNER, |_| None).with_parents(move |pid| parents.get(&pid).copied()),
    );
    Harness {
        manager,
        backend,
        clock,
        data_dir,
    }
}

async fn session(h: &Harness) -> SessionId {
    let session = h
        .manager
        .create(CreateSession {
            shell: Some("/bin/sh".into()),
            cwd: Some("/tmp".into()),
            cols: 80,
            rows: 24,
            origin: SessionOrigin::Ui,
            title: None,
            command: None,
            env: None,
            sandbox: false,
        })
        .await
        .unwrap();
    assert_eq!(session.pid, Some(10_001), "fake root pid for Session 1");
    session.id
}

/// Send one request through the real socket layer over a socketpair.
async fn local(h: &Harness, request: LocalRequest) -> LocalResponse {
    let (mut client, server) = tokio::net::UnixStream::pair().unwrap();
    let manager = h.manager.clone();
    let audit = SecurityAudit::new(&h.data_dir);
    let path = h.data_dir.join("run/sentinald.sock");
    let task = tokio::spawn(async move {
        let handler = sync_handler(move |request| manager.handle_local(request));
        handle_connection(server, OWNER, Some(OWNER), audit, &path, &handler).await
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

fn report(session_id: SessionId, status: AgentStatus, detail: &str) -> LocalRequest {
    LocalRequest::AgentState {
        session_id,
        status,
        detail: Some(detail.into()),
        task: None,
        step: None,
        steps: None,
        eta_secs: None,
        // A client cannot name the caller; the socket layer overwrites this.
        caller_pid: Some(10_001),
    }
}

/// A 6.5 progress report: status WORKING, no detail.
fn progress(
    session_id: SessionId,
    task: Option<&str>,
    step: Option<u32>,
    steps: Option<u32>,
    eta_secs: Option<u32>,
) -> LocalRequest {
    LocalRequest::AgentState {
        session_id,
        status: AgentStatus::Working,
        detail: None,
        task: task.map(Into::into),
        step,
        steps,
        eta_secs,
        caller_pid: None,
    }
}

fn state(h: &Harness, id: SessionId) -> protocol::AgentReportedState {
    *h.manager.session(id).unwrap().agent_state.unwrap()
}

/// Move the injected clock on past the 500 ms report limit.
fn later(h: &Harness, ms: i64) {
    h.clock.fetch_add(ms, Ordering::SeqCst);
}

#[tokio::test]
async fn a_report_from_inside_the_session_is_shown_on_the_session() {
    let h = harness(true);
    let id = session(&h).await;
    let LocalResponse::Session { session } = local(
        &h,
        report(id, AgentStatus::Blocked, "needs approval for cargo test"),
    )
    .await
    else {
        panic!("expected the updated Session");
    };
    let state = session.agent_state.unwrap();
    assert_eq!(state.status, AgentStatus::Blocked);
    assert_eq!(
        state.detail.as_deref(),
        Some("needs approval for cargo test")
    );
    assert_eq!(
        h.manager.session(id).unwrap().agent_state.unwrap().status,
        AgentStatus::Blocked
    );
    // A report never changes the lifecycle state.
    assert_eq!(h.manager.session(id).unwrap().state, SessionState::Idle);
}

#[tokio::test]
async fn a_report_from_outside_the_session_is_forbidden_even_if_it_names_the_root() {
    let h = harness(false);
    let id = session(&h).await;
    let response = local(&h, report(id, AgentStatus::Done, "pretending")).await;
    assert!(
        matches!(
            response,
            LocalResponse::Error {
                code: LocalErrorCode::Forbidden,
                ..
            }
        ),
        "{response:?}"
    );
    assert_eq!(h.manager.session(id).unwrap().agent_state, None);
}

#[tokio::test]
async fn reports_closer_than_500_ms_are_dropped_quietly() {
    let h = harness(true);
    let id = session(&h).await;
    local(&h, report(id, AgentStatus::Working, "first")).await;
    h.clock.fetch_add(200, Ordering::SeqCst);
    let LocalResponse::Session { session } =
        local(&h, report(id, AgentStatus::Done, "too soon")).await
    else {
        panic!("a dropped report still answers with the Session");
    };
    assert_eq!(
        session.agent_state.unwrap().detail.as_deref(),
        Some("first")
    );
    h.clock.fetch_add(400, Ordering::SeqCst);
    local(&h, report(id, AgentStatus::Done, "later")).await;
    assert_eq!(
        h.manager
            .session(id)
            .unwrap()
            .agent_state
            .unwrap()
            .detail
            .as_deref(),
        Some("later")
    );
}

#[tokio::test]
async fn unknown_or_ended_sessions_take_no_reports() {
    let h = harness(true);
    let response = local(&h, report(SessionId(77), AgentStatus::Working, "x")).await;
    assert!(matches!(
        response,
        LocalResponse::Error {
            code: LocalErrorCode::NotFound,
            ..
        }
    ));

    let id = session(&h).await;
    h.manager.kill(id).unwrap();
    let response = local(&h, report(id, AgentStatus::Working, "x")).await;
    assert!(
        matches!(
            response,
            LocalResponse::Error {
                code: LocalErrorCode::InvalidRequest,
                ..
            }
        ),
        "{response:?}"
    );
}

#[tokio::test]
async fn a_bad_detail_is_refused_before_it_reaches_the_session() {
    let h = harness(true);
    let id = session(&h).await;
    let response = local(&h, report(id, AgentStatus::Working, "colour\x1b[31m")).await;
    assert!(matches!(
        response,
        LocalResponse::Error {
            code: LocalErrorCode::InvalidRequest,
            ..
        }
    ));
    assert_eq!(h.manager.session(id).unwrap().agent_state, None);
}

#[tokio::test]
async fn every_session_tells_its_agent_it_may_report() {
    let h = harness(true);
    let id = session(&h).await;
    let env = h.backend.spawn_request(id).unwrap().env;
    assert!(env
        .iter()
        .any(|(k, v)| k == "SENTINAL_AGENT_API" && v == "1"));
    assert!(env
        .iter()
        .any(|(k, v)| k == "SENTINAL_SESSION_ID" && v == &id.0.to_string()));
}

async fn create(h: &Harness, cwd: Option<&str>, sandbox: bool) -> protocol::Session {
    h.manager
        .create(CreateSession {
            shell: Some("/bin/sh".into()),
            cwd: cwd.map(Into::into),
            cols: 80,
            rows: 24,
            origin: SessionOrigin::Watch,
            title: None,
            command: None,
            env: None,
            sandbox,
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn a_sandbox_gets_its_own_private_folder_which_outlives_it() {
    let h = harness(true);
    let session = create(&h, None, true).await;
    let folder = h.data_dir.join("sandbox").join(session.id.0.to_string());
    assert!(session.sandbox);
    assert!(folder.is_dir());
    assert_eq!(std::path::Path::new(&session.cwd), folder);
    assert_eq!(h.backend.spawn_request(session.id).unwrap().cwd, folder);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&folder).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "only the owner can read the sandbox");
    }
    h.manager.kill(session.id).unwrap();
    assert!(folder.is_dir(), "kept for the owner to review");
}

#[tokio::test]
async fn a_sandbox_with_a_cwd_works_there_and_ordinary_sessions_are_unchanged() {
    let h = harness(true);
    let chosen = create(&h, Some("/tmp"), true).await;
    assert!(chosen.sandbox);
    assert_eq!(chosen.cwd, "/tmp");
    assert!(!h
        .data_dir
        .join("sandbox")
        .join(chosen.id.0.to_string())
        .exists());
    let plain = create(&h, Some("/tmp"), false).await;
    assert!(!plain.sandbox);
}

// ---- Milestone 6.5: task, steps and ETA ----

#[tokio::test]
async fn a_progress_report_shows_task_steps_and_the_agents_eta() {
    let h = harness(true);
    let id = session(&h).await;
    let started = h.clock.load(Ordering::SeqCst);
    local(
        &h,
        progress(id, Some("migrate tests"), Some(3), Some(8), Some(720)),
    )
    .await;
    let s = state(&h, id);
    assert_eq!(s.task.as_deref(), Some("migrate tests"));
    assert_eq!((s.step, s.steps, s.eta_secs), (Some(3), Some(8), Some(720)));
    assert_eq!(s.task_started_at, Some(Timestamp(started)));
    assert_eq!(s.estimated_left_secs, None, "one step is not a pace");
}

#[tokio::test]
async fn a_new_task_resets_time_on_task_and_the_same_task_keeps_it() {
    let h = harness(true);
    let id = session(&h).await;
    let first = h.clock.load(Ordering::SeqCst);
    local(
        &h,
        progress(id, Some("migrate tests"), Some(1), Some(8), None),
    )
    .await;
    later(&h, 60_000);
    local(&h, progress(id, Some("migrate tests"), Some(2), None, None)).await;
    assert_eq!(state(&h, id).task_started_at, Some(Timestamp(first)));
    later(&h, 60_000);
    let second = h.clock.load(Ordering::SeqCst);
    local(
        &h,
        progress(id, Some("update docs"), Some(1), Some(3), None),
    )
    .await;
    let s = state(&h, id);
    assert_eq!(s.task_started_at, Some(Timestamp(second)));
    assert_eq!((s.step, s.steps), (Some(1), Some(3)));
    assert_eq!(s.estimated_left_secs, None, "a new task starts a new pace");
}

#[tokio::test]
async fn a_report_keeps_the_progress_it_does_not_mention() {
    let h = harness(true);
    let id = session(&h).await;
    local(
        &h,
        progress(id, Some("migrate tests"), Some(3), Some(8), Some(720)),
    )
    .await;
    later(&h, 1_000);
    local(
        &h,
        report(id, AgentStatus::Blocked, "needs the staging key"),
    )
    .await;
    let s = state(&h, id);
    assert_eq!(s.status, AgentStatus::Blocked);
    assert_eq!(s.detail.as_deref(), Some("needs the staging key"));
    assert_eq!(s.task.as_deref(), Some("migrate tests"));
    assert_eq!((s.step, s.steps), (Some(3), Some(8)));
    assert_eq!(s.eta_secs, Some(720), "same step, the ETA still stands");
    later(&h, 1_000);
    local(&h, progress(id, None, Some(4), None, None)).await;
    let s = state(&h, id);
    assert_eq!(s.detail, None, "detail is always replaced");
    assert_eq!(
        s.eta_secs, None,
        "a new step without an ETA drops the old one"
    );
}

#[tokio::test]
async fn the_pace_estimate_is_the_median_step_time_times_the_steps_left() {
    let h = harness(true);
    let id = session(&h).await;
    local(
        &h,
        progress(id, Some("migrate tests"), Some(1), Some(8), None),
    )
    .await;
    // Steps take 60 s, 120 s (an outlier) and 60 s: the median is 60 s.
    later(&h, 60_000);
    local(&h, progress(id, None, Some(2), None, None)).await;
    assert_eq!(state(&h, id).estimated_left_secs, Some(6 * 60));
    later(&h, 120_000);
    local(&h, progress(id, None, Some(3), None, None)).await;
    later(&h, 60_000);
    local(&h, progress(id, None, Some(4), None, None)).await;
    assert_eq!(state(&h, id).estimated_left_secs, Some(4 * 60));
    // A changed plan or a step going backwards forgets the pace.
    later(&h, 1_000);
    local(&h, progress(id, None, Some(4), Some(12), None)).await;
    assert_eq!(state(&h, id).estimated_left_secs, None);
    later(&h, 30_000);
    local(&h, progress(id, None, Some(5), None, None)).await;
    assert_eq!(state(&h, id).estimated_left_secs, Some(7 * 30));
    later(&h, 1_000);
    local(&h, progress(id, None, Some(2), None, None)).await;
    assert_eq!(state(&h, id).estimated_left_secs, None);
}

#[tokio::test]
async fn a_step_past_the_plan_after_merging_is_refused() {
    let h = harness(true);
    let id = session(&h).await;
    local(
        &h,
        progress(id, Some("migrate tests"), Some(3), Some(8), None),
    )
    .await;
    later(&h, 1_000);
    let response = local(&h, progress(id, None, Some(9), None, None)).await;
    assert!(
        matches!(
            response,
            LocalResponse::Error {
                code: LocalErrorCode::InvalidRequest,
                ..
            }
        ),
        "{response:?}"
    );
    assert_eq!(state(&h, id).step, Some(3));
}

#[tokio::test]
async fn a_quick_last_report_is_applied_when_the_500_ms_window_ends() {
    let h = harness(true);
    let id = session(&h).await;
    local(
        &h,
        progress(id, Some("migrate tests"), Some(8), Some(8), None),
    )
    .await;
    later(&h, 100);
    local(&h, report(id, AgentStatus::Done, "all green")).await;
    assert_eq!(
        state(&h, id).status,
        AgentStatus::Working,
        "held, not shown yet"
    );
    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
    let s = state(&h, id);
    assert_eq!(s.status, AgentStatus::Done);
    assert_eq!(s.detail.as_deref(), Some("all green"));
    assert_eq!(s.step, Some(8));
}

#[tokio::test]
async fn progress_never_changes_the_lifecycle_state() {
    let h = harness(true);
    let id = session(&h).await;
    local(
        &h,
        progress(id, Some("migrate tests"), Some(8), Some(8), None),
    )
    .await;
    later(&h, 1_000);
    local(&h, report(id, AgentStatus::Done, "finished")).await;
    assert_eq!(h.manager.session(id).unwrap().state, SessionState::Idle);
}

async fn eventually(h: &Harness, id: SessionId, ok: impl Fn(&protocol::Session) -> bool) {
    for _ in 0..200 {
        if ok(&h.manager.session(id).unwrap()) {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!(
        "timed out: {:?}",
        h.manager.session(id).unwrap().agent_state
    );
}

#[tokio::test]
async fn a_scripts_task_and_state_lines_show_like_a_report_and_store_no_event() {
    let h = harness(false);
    let id = session(&h).await;
    h.backend
        .output(
            id,
            b"::sentinal task name=\"migrate tests\" step=3 steps=8 eta=12m\r\n",
        )
        .unwrap();
    eventually(&h, id, |s| s.agent_state.is_some()).await;
    let s = state(&h, id);
    assert_eq!(s.status, AgentStatus::Working);
    assert_eq!(s.task.as_deref(), Some("migrate tests"));
    assert_eq!((s.step, s.steps, s.eta_secs), (Some(3), Some(8), Some(720)));

    // A state line within 500 ms is held and applied on the next output.
    h.backend
        .output(id, b"::sentinal state value=blocked\r\n")
        .unwrap();
    tokio::time::sleep(std::time::Duration::from_millis(40)).await;
    assert_eq!(state(&h, id).status, AgentStatus::Working);
    later(&h, 1_000);
    h.backend.output(id, b"still going\r\n").unwrap();
    eventually(&h, id, |s| {
        s.agent_state
            .as_ref()
            .is_some_and(|a| a.status == AgentStatus::Blocked)
    })
    .await;
    let s = state(&h, id);
    assert_eq!(s.step, Some(3), "a state line keeps the progress");
    assert_eq!(h.manager.session(id).unwrap().state, SessionState::Idle);
}
