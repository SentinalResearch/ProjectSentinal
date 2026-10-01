//! In-process Axum API and WebSocket dispatcher behaviour tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::{to_bytes, Body},
    extract::connect_info::MockConnectInfo,
    http::{header::AUTHORIZATION, Request, StatusCode},
};
use daemon::{
    api::{router, AppState, WsAction},
    audit::AuditLog,
    auth::AuthManager,
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{
    AuthFailure, AuthFailureCode, AuthSession, AuthSessionInfo, AuthStatus, ClientRequest, Command,
    CommandId, DaemonMessage, Envelope, Event, EventId, EventKind, Machine, MachineId,
    MachineStatus, Os, SessionId, Timestamp,
};
use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use store::Store;
use tower::ServiceExt;

static NEXT_STATE: AtomicU64 = AtomicU64::new(1);

fn state() -> AppState {
    state_with_store().0
}

fn state_with_store() -> (AppState, Arc<Store>) {
    let data_dir = std::env::temp_dir().join(format!(
        "terminal-agent-api-test-{}-{}",
        std::process::id(),
        NEXT_STATE.fetch_add(1, Ordering::Relaxed)
    ));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let manager = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 100,
            clock: Arc::new(|| Timestamp(1_790_000_000_000)),
        },
        AuditLog::new(&data_dir),
        store.clone(),
    )
    .unwrap();
    let state = AppState::new(
        manager,
        Machine {
            id: MachineId("test-machine".into()),
            name: "Test Machine".into(),
            os: Os::Linux,
            cpu: "test cpu".into(),
            cpu_cores: 8,
            ram_bytes: 16 * 1024 * 1024 * 1024,
            gpus: vec![],
            status: MachineStatus::Online,
            last_seen: Timestamp(1_790_000_000_000),
        },
        AuthManager::load(&data_dir.join("auth.json"), "correct-token".into()).unwrap(),
    );
    (state, store)
}

#[tokio::test]
async fn history_and_full_text_search_return_stored_commands() {
    let (state, store) = state_with_store();
    store
        .insert_command(&Command::start(
            CommandId(1),
            SessionId(7),
            MachineId("test-machine".into()),
            "python train.py",
            "/work",
            Timestamp(100),
        ))
        .unwrap();
    store
        .insert_command(&Command::start(
            CommandId(2),
            SessionId(8),
            MachineId("test-machine".into()),
            "cargo build",
            "/work",
            Timestamp(200),
        ))
        .unwrap();

    let history = state
        .dispatch_text(
            &Envelope::new(ClientRequest::GetHistory {
                session_id: None,
                limit: 10,
            })
            .encode()
            .unwrap(),
        )
        .await;
    assert!(
        matches!(history, WsAction::Reply(DaemonMessage::History { commands }) if commands.len() == 2)
    );

    let search = state
        .dispatch_text(
            &Envelope::new(ClientRequest::SearchCommands {
                query: "train".into(),
                limit: 10,
            })
            .encode()
            .unwrap(),
        )
        .await;
    assert!(
        matches!(search, WsAction::Reply(DaemonMessage::History { commands }) if commands.len() == 1 && commands[0].text == "python train.py")
    );
}

#[tokio::test]
async fn event_replay_is_filtered_bounded_and_oldest_first() {
    let (state, store) = state_with_store();
    for (id, session_id) in [(1, 7), (2, 8), (3, 7)] {
        store
            .insert_event(&Event {
                id: EventId(id),
                session_id: SessionId(session_id),
                command_id: None,
                at: Timestamp(id as i64),
                kind: EventKind::WarningDetected {
                    message: format!("event {id}"),
                },
            })
            .unwrap();
    }
    let action = state
        .dispatch_text(
            &Envelope::new(ClientRequest::GetEvents {
                session_id: Some(SessionId(7)),
                after: Some(EventId(1)),
                limit: 1,
            })
            .encode()
            .unwrap(),
        )
        .await;
    assert!(matches!(
        action,
        WsAction::Reply(DaemonMessage::Events { events })
            if events.iter().map(|event| event.id).collect::<Vec<_>>() == vec![EventId(3)]
    ));
}

#[tokio::test]
async fn health_is_public_and_reports_protocol_version() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    assert_eq!(body.as_ref(), br#"{"ok":true,"version":1}"#);
}

#[tokio::test]
async fn missing_token_is_unauthorized() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/v1/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_token_is_unauthorized() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/v1/sessions")
                .header(AUTHORIZATION, "Bearer wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn websocket_query_token_passes_authentication() {
    let client: SocketAddr = "100.64.0.2:5000".parse().unwrap();
    let response = router(state())
        .layer(MockConnectInfo(client))
        .oneshot(
            Request::builder()
                .uri("/api/v1/ws?token=correct-token")
                .header("connection", "upgrade")
                .header("upgrade", "websocket")
                .header("sec-websocket-version", "13")
                .header("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ==")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);

    let unauthorized = router(state())
        .layer(MockConnectInfo(client))
        .oneshot(
            Request::builder()
                .uri("/api/v1/ws?token=wrong-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn bearer_token_authorizes_sessions() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .uri("/api/v1/sessions")
                .header(AUTHORIZATION, "Bearer correct-token")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let envelope: Envelope<DaemonMessage> = serde_json::from_slice(&body).unwrap();
    assert!(matches!(
        envelope.payload,
        DaemonMessage::Sessions { sessions } if sessions.is_empty()
    ));
}

#[tokio::test]
async fn password_setup_login_logout_and_browser_authorization_work() {
    let client: SocketAddr = "100.64.0.2:5000".parse().unwrap();
    let app = router(state()).layer(MockConnectInfo(client));

    let status = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let body = to_bytes(status.into_body(), usize::MAX).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<AuthStatus>(&body).unwrap(),
        AuthStatus {
            setup_required: true
        }
    );

    let setup = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/setup")
                .header(AUTHORIZATION, "Bearer correct-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"password":"correct horse battery staple"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(setup.status(), StatusCode::OK);
    let body = to_bytes(setup.into_body(), usize::MAX).await.unwrap();
    let setup_session: AuthSession = serde_json::from_slice(&body).unwrap();

    let session_info = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/auth/session")
                .header(AUTHORIZATION, format!("Bearer {}", setup_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(session_info.status(), StatusCode::OK);
    let body = to_bytes(session_info.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(
        serde_json::from_slice::<AuthSessionInfo>(&body)
            .unwrap()
            .expires_at,
        setup_session.expires_at
    );

    let browser_access = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/sessions")
                .header(AUTHORIZATION, format!("Bearer {}", setup_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(browser_access.status(), StatusCode::OK);

    let logout = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/logout")
                .header(AUTHORIZATION, format!("Bearer {}", setup_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);

    let logged_out = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/sessions")
                .header(AUTHORIZATION, format!("Bearer {}", setup_session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(logged_out.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn login_failure_is_structured_and_connection_can_retry() {
    let client: SocketAddr = "100.64.0.3:5000".parse().unwrap();
    let app = router(state()).layer(MockConnectInfo(client));
    let setup = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/setup")
                .header(AUTHORIZATION, "Bearer correct-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"password":"correct horse battery staple"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(setup.status(), StatusCode::OK);

    let wrong = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"password":"wrong password"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(wrong.into_body(), usize::MAX).await.unwrap();
    let failure: AuthFailure = serde_json::from_slice(&body).unwrap();
    assert_eq!(failure.code, AuthFailureCode::InvalidCredentials);
    assert_eq!(failure.attempts_remaining, Some(4));

    let correct = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/login")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"password":"correct horse battery staple"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(correct.status(), StatusCode::OK);
}

#[tokio::test]
async fn setup_rejects_the_wrong_bootstrap_token() {
    let response = router(state())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/setup")
                .header(AUTHORIZATION, "Bearer wrong-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"password":"correct horse battery staple"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let failure: AuthFailure = serde_json::from_slice(&body).unwrap();
    assert_eq!(failure.code, AuthFailureCode::InvalidBootstrap);
}

#[tokio::test]
async fn changing_password_revokes_the_previous_browser_session() {
    let client: SocketAddr = "100.64.0.4:5000".parse().unwrap();
    let app = router(state()).layer(MockConnectInfo(client));
    let setup = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/setup")
                .header(AUTHORIZATION, "Bearer correct-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"password":"correct horse battery staple"}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(setup.into_body(), usize::MAX).await.unwrap();
    let old: AuthSession = serde_json::from_slice(&body).unwrap();

    let changed = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/auth/password")
                .header(AUTHORIZATION, format!("Bearer {}", old.token))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "current_password":"correct horse battery staple",
                        "new_password":"a different secure passphrase"
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(changed.status(), StatusCode::OK);
    let body = to_bytes(changed.into_body(), usize::MAX).await.unwrap();
    let new: AuthSession = serde_json::from_slice(&body).unwrap();

    let old_access = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/v1/sessions")
                .header(AUTHORIZATION, format!("Bearer {}", old.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let new_access = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/sessions")
                .header(AUTHORIZATION, format!("Bearer {}", new.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(old_access.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(new_access.status(), StatusCode::OK);
}

#[tokio::test]
async fn bad_json_returns_error_then_dispatcher_accepts_next_message() {
    let state = state();

    let bad = state.dispatch_text("{not json").await;
    let good = state
        .dispatch_text(&Envelope::new(ClientRequest::ListSessions).encode().unwrap())
        .await;

    assert!(matches!(
        bad,
        WsAction::Reply(DaemonMessage::Error { message }) if message.contains("invalid JSON")
    ));
    assert!(matches!(
        good,
        WsAction::Reply(DaemonMessage::Sessions { sessions }) if sessions.is_empty()
    ));
}

#[tokio::test]
async fn local_only_request_is_rejected_remotely_without_mutating_state() {
    let state = state();
    let local = serde_json::json!({
        "version": 1,
        "payload": {
            "type": "ADOPT_PROCESS",
            "pid": 4242,
            "title": "must not exist",
            "log_path": null
        }
    });

    assert!(matches!(
        state.dispatch_text(&local.to_string()).await,
        WsAction::Reply(DaemonMessage::Error { .. })
    ));
    assert!(state.sessions.list().unwrap().is_empty());

    let valid = Envelope::new(ClientRequest::ListSessions).encode().unwrap();
    assert!(matches!(
        state.dispatch_text(&valid).await,
        WsAction::Reply(DaemonMessage::Sessions { sessions }) if sessions.is_empty()
    ));
}

#[tokio::test]
async fn unsupported_version_returns_error() {
    let action = state()
        .dispatch_text(r#"{"version":99,"payload":{"type":"LIST_SESSIONS"}}"#)
        .await;

    assert!(matches!(
        action,
        WsAction::Reply(DaemonMessage::Error { message })
            if message.contains("unsupported protocol version 99")
    ));
}

#[tokio::test]
async fn invalid_request_returns_error_without_dispatching() {
    let request = Envelope::new(ClientRequest::Resize {
        session_id: protocol::SessionId(1),
        cols: 0,
        rows: 24,
    })
    .encode()
    .unwrap();

    let action = state().dispatch_text(&request).await;

    assert!(matches!(
        action,
        WsAction::Reply(DaemonMessage::Error { message })
            if message.contains("terminal size must be at least 1x1")
    ));
}
