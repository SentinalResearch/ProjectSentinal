//! Enrollment routes on the full daemon router: owner-only pairing, origins,
//! password interplay, Tailscale Serve identity, and the worker identity route.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::{to_bytes, Body},
    extract::connect_info::MockConnectInfo,
    http::{HeaderMap, Request, StatusCode},
    Router,
};
use daemon::{
    api::{router, AppState},
    audit::AuditLog,
    auth::AuthManager,
    control::{ControlService, ControlSettings},
    origin::OriginPolicy,
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{
    link::{PairingCode, PairingCodeState, PairingCodeStatus, RegistryMachines},
    Machine, MachineId, MachineStatus, Os, Timestamp,
};
use serde_json::{json, Value};
use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use store::Store;
use tower::ServiceExt;

const TOKEN: &str = "machine-token-for-tests";
const PASSWORD: &str = "a-long-owner-password";
const ORIGIN: &str = "https://sentinal-control.example.ts.net";

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Daemon {
    app: Router,
    dir: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn daemon(control: bool, behind_serve: bool, peer: &str) -> Daemon {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-enroll-api-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let sessions = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId("01J9Y0CTRL0000000000000000".into()),
            data_dir: dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(1)),
        },
        AuditLog::new(&dir),
        store.clone(),
    )
    .unwrap();
    let control = control.then(|| {
        ControlService::open(
            store,
            ControlSettings {
                public_url: ORIGIN.into(),
                heartbeat_interval_ms: 30_000,
                development: false,
                machine_id: MachineId("01J9Y0CTRL0000000000000000".into()),
                display_name: "Control".into(),
                os: Os::Macos,
                daemon_version: "0.0.1".into(),
                secrets_file: dir.join("machine-credentials.json"),
                audit_dir: dir.clone(),
            },
            Arc::new(|| Timestamp(1_790_900_000_000)),
        )
        .unwrap()
    });
    let machine = Machine {
        id: MachineId("01J9Y0CTRL0000000000000000".into()),
        name: "control".into(),
        os: Os::Macos,
        cpu: "c".into(),
        cpu_cores: 1,
        ram_bytes: 1,
        gpus: vec![],
        status: MachineStatus::Online,
        last_seen: Timestamp(1),
        role: None,
    };
    let auth = AuthManager::load(&dir.join("auth.json"), TOKEN.into()).unwrap();
    let state = AppState::new(sessions, machine, auth)
        .with_origins(OriginPolicy::new(&[ORIGIN.to_string()], false).unwrap())
        .with_control(control)
        .with_tailscale_serve(behind_serve);
    let peer: SocketAddr = peer.parse().unwrap();
    Daemon {
        app: router(state).layer(MockConnectInfo(peer)),
        dir,
    }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

impl Daemon {
    async fn send(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        headers: &[(&str, &str)],
        body: Option<Value>,
    ) -> Reply {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
        }
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        let request = match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string())),
            None => request.body(Body::empty()),
        }
        .unwrap();
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        Reply {
            status,
            headers,
            body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        }
    }

    async fn owner_session(&self) -> String {
        let setup = self
            .send(
                "POST",
                "/api/v1/auth/setup",
                Some(TOKEN),
                &[],
                Some(json!({ "password": PASSWORD })),
            )
            .await;
        assert_eq!(setup.status, StatusCode::OK);
        setup.body["token"].as_str().unwrap().to_string()
    }
}

#[tokio::test]
async fn pairing_codes_need_an_owner_browser_session_not_the_machine_token() {
    let d = daemon(true, false, "100.64.0.2:5000");
    assert_eq!(
        d.send("POST", "/api/v1/pairing/codes", None, &[], None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    let session = d.owner_session().await;
    let with_token = d
        .send("POST", "/api/v1/pairing/codes", Some(TOKEN), &[], None)
        .await;
    assert_eq!(with_token.status, StatusCode::FORBIDDEN);

    let created = d
        .send("POST", "/api/v1/pairing/codes", Some(&session), &[], None)
        .await;

    assert_eq!(created.status, StatusCode::OK);
    let code: PairingCode = serde_json::from_value(created.body).unwrap();
    assert!(!code.connect_command.contains(code.code.expose()));
    let status = d
        .send(
            "GET",
            &format!("/api/v1/pairing/codes/{}", code.code_id),
            Some(&session),
            &[],
            None,
        )
        .await;
    let status: PairingCodeStatus = serde_json::from_value(status.body).unwrap();
    assert_eq!(status.state, PairingCodeState::Waiting);
    assert!(!serde_json::to_string(&status)
        .unwrap()
        .contains(code.code.expose()));
    let registry = d
        .send("GET", "/api/v1/machines", Some(&session), &[], None)
        .await;
    let registry: RegistryMachines = serde_json::from_value(registry.body).unwrap();
    assert_eq!(registry.machines.len(), 1, "the control registers itself");
}

#[tokio::test]
async fn control_projection_snapshot_is_one_bounded_owner_resource() {
    let d = daemon(true, false, "100.64.0.2:5000");
    assert_eq!(
        d.send("GET", "/api/v1/control/snapshot", None, &[], None)
            .await
            .status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        d.send("GET", "/api/v1/control/snapshot", Some(TOKEN), &[], None,)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    let session = d.owner_session().await;
    let response = d
        .send("GET", "/api/v1/control/snapshot", Some(&session), &[], None)
        .await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(
        response.headers.get("cache-control").unwrap(),
        "no-store, private"
    );
    assert_eq!(
        response.headers.get("x-content-type-options").unwrap(),
        "nosniff"
    );
    let snapshot: protocol::ControlProjectionSnapshot =
        serde_json::from_value(response.body).unwrap();
    assert_eq!(snapshot.machines.len(), 1);
    assert_eq!(
        snapshot.machines[0].freshness,
        protocol::ProjectionFreshness::Stale
    );
    assert!(snapshot.machines[0].sessions.is_empty());
    assert!(!snapshot.machines[0].projection_available);
}

#[tokio::test]
async fn a_daemon_without_the_control_role_answers_not_control() {
    let d = daemon(false, false, "100.64.0.2:5000");
    let session = d.owner_session().await;
    for (method, path) in [
        ("POST", "/api/v1/pairing/codes"),
        ("GET", "/api/v1/machines"),
        ("POST", "/api/v1/link/exchange"),
    ] {
        let body = (method == "POST" && path.contains("exchange")).then(|| json!({}));
        let reply = d.send(method, path, Some(&session), &[], body).await;
        if path.contains("exchange") {
            assert!(reply.status.is_client_error(), "{path}");
        } else {
            assert_eq!(reply.status, StatusCode::NOT_FOUND, "{path}");
            assert_eq!(reply.body["code"], "NOT_CONTROL");
        }
    }
}

#[tokio::test]
async fn origins_are_enforced_on_every_route_including_websocket_upgrades() {
    let d = daemon(true, false, "100.64.0.2:5000");
    let session = d.owner_session().await;
    let evil = [("origin", "https://sentinal-control.example.ts.net.evil.com")];
    for path in ["/api/v1/auth/status", "/api/v1/health", "/api/v1/machines"] {
        let reply = d.send("GET", path, Some(&session), &evil, None).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(reply.body["code"], "ORIGIN_REJECTED");
    }
    let upgrade = [
        ("origin", "null"),
        ("connection", "upgrade"),
        ("upgrade", "websocket"),
        ("sec-websocket-version", "13"),
        ("sec-websocket-key", "dGhlIHNhbXBsZSBub25jZQ=="),
    ];
    let ws = d
        .send(
            "GET",
            &format!("/api/v1/ws?token={session}"),
            None,
            &upgrade,
            None,
        )
        .await;
    assert_eq!(ws.status, StatusCode::FORBIDDEN);
    let good = d
        .send(
            "GET",
            "/api/v1/machines",
            Some(&session),
            &[("origin", ORIGIN)],
            None,
        )
        .await;
    assert_eq!(good.status, StatusCode::OK);
}

#[tokio::test]
async fn changing_the_owner_password_cancels_outstanding_codes() {
    let d = daemon(true, false, "100.64.0.2:5000");
    let session = d.owner_session().await;
    let created = d
        .send("POST", "/api/v1/pairing/codes", Some(&session), &[], None)
        .await;
    let code_id = created.body["code_id"].as_str().unwrap().to_string();

    let changed = d
        .send(
            "POST",
            "/api/v1/auth/password",
            Some(&session),
            &[],
            Some(json!({ "current_password": PASSWORD, "new_password": "another-long-password" })),
        )
        .await;
    assert_eq!(changed.status, StatusCode::OK);
    let fresh = changed.body["token"].as_str().unwrap();

    let status = d
        .send(
            "GET",
            &format!("/api/v1/pairing/codes/{code_id}"),
            Some(fresh),
            &[],
            None,
        )
        .await;
    assert_eq!(status.body["state"], "CANCELLED");
}

async fn fail_logins(d: &Daemon, user: &str, times: usize) {
    for _ in 0..times {
        d.send(
            "POST",
            "/api/v1/auth/login",
            None,
            &[("tailscale-user-login", user)],
            Some(json!({ "password": "wrong-password-xx" })),
        )
        .await;
    }
}

async fn login_status(d: &Daemon, user: &str) -> StatusCode {
    d.send(
        "POST",
        "/api/v1/auth/login",
        None,
        &[("tailscale-user-login", user)],
        Some(json!({ "password": PASSWORD })),
    )
    .await
    .status
}

#[tokio::test]
async fn behind_tailscale_serve_one_users_lockout_does_not_lock_out_another() {
    let d = daemon(false, true, "127.0.0.1:5000");
    d.owner_session().await;
    fail_logins(&d, "mallory@example.com", 5).await;

    assert_eq!(
        login_status(&d, "mallory@example.com").await,
        StatusCode::LOCKED
    );
    assert_eq!(login_status(&d, "owner@example.com").await, StatusCode::OK);
}

#[tokio::test]
async fn the_tailscale_header_is_ignored_unless_configured_and_from_loopback() {
    for (behind_serve, peer) in [(false, "127.0.0.1:5000"), (true, "100.64.0.9:5000")] {
        let d = daemon(false, behind_serve, peer);
        d.owner_session().await;
        fail_logins(&d, "mallory@example.com", 5).await;
        assert_eq!(
            login_status(&d, "owner@example.com").await,
            StatusCode::LOCKED,
            "spoofed header must not dodge the lockout ({behind_serve}, {peer})"
        );
    }
}

#[tokio::test]
async fn link_identity_requires_the_control_credential() {
    let d = daemon(false, false, "100.64.0.2:5000");
    let reply = d
        .send(
            "GET",
            "/api/v1/link/identity",
            Some(&"a".repeat(64)),
            &[],
            None,
        )
        .await;
    assert_eq!(reply.status, StatusCode::UNAUTHORIZED);
}
