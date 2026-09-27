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
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{
    ClientRequest, DaemonMessage, Envelope, Machine, MachineId, MachineStatus, Os, Timestamp,
};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower::ServiceExt;

fn state() -> AppState {
    let data_dir = PathBuf::from("/tmp/terminal-agent-api-test");
    let manager = SessionManager::new(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 100,
            clock: Arc::new(|| Timestamp(1_790_000_000_000)),
        },
        AuditLog::new(&data_dir),
    );
    AppState::new(
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
        "correct-token".into(),
    )
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
