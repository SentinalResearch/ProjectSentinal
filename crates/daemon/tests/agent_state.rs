//! Milestone 6.4: an agent inside a Session reports what it is doing over the
//! private local socket, and only a process inside that Session may.

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
        // A client cannot name the caller; the socket layer overwrites this.
        caller_pid: Some(10_001),
    }
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
