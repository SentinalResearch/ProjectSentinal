//! Authenticated persisted terminal-output replay tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::{to_bytes, Body},
    http::{header::AUTHORIZATION, Request, StatusCode},
};
use daemon::{
    api::{router, AppState},
    audit::AuditLog,
    auth::AuthManager,
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{
    CaptureLevel, Machine, MachineId, MachineStatus, Os, Session, SessionId, SessionOrigin,
    SessionState, Timestamp,
};
use std::{
    fs,
    io::Cursor,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use store::Store;
use tower::ServiceExt;

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

fn test_state(name: &str, with_session: bool) -> (AppState, PathBuf) {
    let data_dir = std::env::temp_dir().join(format!(
        "sentinal-output-replay-{name}-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(data_dir.join("sessions")).unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    if with_session {
        let mut session = Session::new(
            SessionId(7),
            MachineId("test-machine".into()),
            "/bin/zsh",
            "/safe/work",
            Timestamp(100),
        );
        session.state = SessionState::Completed;
        session.exit_code = Some(0);
        session.updated_at = Timestamp(200);
        store.insert_session(&session).unwrap();
    }
    let manager = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 100,
            clock: Arc::new(|| Timestamp(300)),
        },
        AuditLog::new(&data_dir),
        store,
    )
    .unwrap();
    let machine = Machine {
        id: MachineId("test-machine".into()),
        name: "Test Machine".into(),
        os: Os::Linux,
        cpu: "test cpu".into(),
        cpu_cores: 8,
        ram_bytes: 1024,
        gpus: vec![],
        status: MachineStatus::Online,
        last_seen: Timestamp(300),
    };
    let auth = AuthManager::load(&data_dir.join("auth.json"), "correct-token".into()).unwrap();
    let state = AppState::new(manager, machine, auth);
    if with_session {
        let listed = state.sessions.list().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, SessionId(7));
        assert_eq!(listed[0].state, SessionState::Completed);
    }
    (state, data_dir)
}

fn frame(bytes: &[u8]) -> Vec<u8> {
    zstd::stream::encode_all(Cursor::new(bytes), 0).unwrap()
}

#[test]
fn persisted_finished_session_rejects_input_without_a_pty_or_audit_record() {
    let (state, data_dir) = test_state("persisted-read-only", true);

    let error = state
        .sessions
        .write_input(SessionId(7), b"secret payload", "remote-client")
        .unwrap_err();

    assert!(error.to_string().contains("terminal state Completed"));
    assert!(!data_dir.join("audit.jsonl").exists());
    assert!(!data_dir.join("sessions/7.log.zst").exists());
    fs::remove_dir_all(data_dir).unwrap();
}

async fn get(state: AppState, uri: &str, authorized: bool) -> axum::response::Response {
    let mut request = Request::builder().uri(uri);
    if authorized {
        request = request.header(AUTHORIZATION, "Bearer correct-token");
    }
    router(state)
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn replays_a_bounded_page_from_a_valid_multiframe_log_after_restart() {
    let (state, data_dir) = test_state("multi-frame", true);
    let mut compressed = frame(b"first-");
    compressed.extend(frame(b"second"));
    fs::write(data_dir.join("sessions/7.log.zst"), compressed).unwrap();

    let response = get(state, "/api/v1/sessions/7/output?offset=6&limit=6", true).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/octet-stream"
    );
    assert_eq!(response.headers()["x-sentinal-output-offset"], "6");
    assert_eq!(response.headers()["x-sentinal-output-next-offset"], "12");
    assert_eq!(response.headers()["x-sentinal-output-complete"], "true");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.as_ref(), b"second");
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn empty_log_is_a_complete_empty_replay() {
    let (state, data_dir) = test_state("empty", true);
    fs::write(data_dir.join("sessions/7.log.zst"), frame(b"")).unwrap();

    let response = get(state, "/api/v1/sessions/7/output", true).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-sentinal-output-complete"], "true");
    assert!(to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .is_empty());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn missing_unknown_unauthorized_and_traversal_requests_are_rejected() {
    let (state, data_dir) = test_state("errors", true);

    let missing = get(state.clone(), "/api/v1/sessions/7/output", true).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let missing_body = to_bytes(missing.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&missing_body).unwrap()["code"],
        "OUTPUT_NOT_FOUND"
    );

    let unknown = get(state.clone(), "/api/v1/sessions/99/output", true).await;
    assert_eq!(unknown.status(), StatusCode::NOT_FOUND);
    let unknown_body = to_bytes(unknown.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&unknown_body).unwrap()["code"],
        "UNKNOWN_SESSION"
    );

    let unauthorized = get(state.clone(), "/api/v1/sessions/7/output", false).await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let traversal = get(state, "/api/v1/sessions/%2e%2e/output", true).await;
    assert!(!traversal.status().is_success());
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn replay_range_and_response_are_strictly_bounded() {
    let (state, data_dir) = test_state("bounds", true);
    fs::write(
        data_dir.join("sessions/7.log.zst"),
        frame(&vec![b'x'; 1_048_577]),
    )
    .unwrap();

    let too_large = get(
        state.clone(),
        "/api/v1/sessions/7/output?offset=67108864&limit=1",
        true,
    )
    .await;
    assert_eq!(too_large.status(), StatusCode::BAD_REQUEST);

    let capped = get(
        state,
        "/api/v1/sessions/7/output?offset=0&limit=1048576",
        true,
    )
    .await;
    assert_eq!(capped.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(capped.into_body(), 1_048_577).await.unwrap().len(),
        1_048_576
    );
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn truncated_final_frame_returns_preceding_output_with_a_warning() {
    let (state, data_dir) = test_state("truncated", true);
    let mut compressed = frame(b"verified preceding output\n");
    let final_frame = frame(b"unfinished final output");
    compressed.extend(&final_frame[..final_frame.len() / 2]);
    fs::write(data_dir.join("sessions/7.log.zst"), compressed).unwrap();

    let response = get(state, "/api/v1/sessions/7/output", true).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["x-sentinal-output-warning"],
        "TRUNCATED_ZSTD"
    );
    assert_eq!(response.headers()["x-sentinal-output-complete"], "true");
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert!(body.starts_with(b"verified preceding output\n"));
    fs::remove_dir_all(data_dir).unwrap();
}

#[tokio::test]
async fn reduced_capture_cannot_read_a_daemon_terminal_log() {
    let (state, data_dir) = test_state("reduced", false);
    let mut session = Session::new(
        SessionId(7),
        MachineId("test-machine".into()),
        "/proc/7/exe",
        "/safe/work",
        Timestamp(100),
    );
    session.origin = SessionOrigin::Adopted;
    session.capture = CaptureLevel::Process;
    session.pid = Some(7);
    session.state = SessionState::Running;
    // Rebuild state with its store populated, as a daemon restart would.
    let store = Arc::new(Store::open_in_memory().unwrap());
    store.insert_session(&session).unwrap();
    let manager = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 100,
            clock: Arc::new(|| Timestamp(300)),
        },
        AuditLog::new(&data_dir),
        store,
    )
    .unwrap();
    let denied = manager.replay_output(SessionId(7), 0, 16).unwrap_err();
    assert!(matches!(
        denied,
        daemon::session_manager::SessionManagerError::OutputUnavailable {
            session_id: SessionId(7),
            capture: CaptureLevel::Process,
        }
    ));
    drop(state);
    fs::remove_dir_all(data_dir).unwrap();
}
