//! Owner-authenticated native Push HTTP resources.

#![allow(clippy::unwrap_used)]

use axum::{
    body::{to_bytes, Body},
    extract::connect_info::MockConnectInfo,
    http::{Request, StatusCode},
};
use base64ct::{Base64UrlUnpadded, Encoding as _};
use daemon::{
    api::{router, AppState},
    audit::AuditLog,
    auth::AuthManager,
    control::{ControlService, ControlSettings},
    origin::OriginPolicy,
    pty::FakePty,
    push::gateway::{PushGateway, PushGatewaySettings},
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{Machine, MachineId, MachineStatus, Os, Timestamp};
use serde_json::{json, Value};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr},
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use store::Store;
use tower::ServiceExt;
use web_push_native::p256::{elliptic_curve::sec1::ToEncodedPoint, SecretKey};

static NEXT: AtomicU64 = AtomicU64::new(1);
const TOKEN: &str = "machine-token-for-push-tests";
const ORIGIN: &str = "https://control.example.ts.net";

struct Harness {
    app: axum::Router,
    dir: PathBuf,
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn harness() -> Harness {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-push-api-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let uid = fs::metadata(&dir).unwrap().uid();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let machine_id = MachineId("01J9Y0CTRL0000000000000000".into());
    let sessions = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: machine_id.clone(),
            data_dir: dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(1)),
        },
        AuditLog::new(&dir),
        store.clone(),
    )
    .unwrap();
    let control = ControlService::open(
        store.clone(),
        ControlSettings {
            public_url: ORIGIN.into(),
            heartbeat_interval_ms: 30_000,
            development: false,
            machine_id: machine_id.clone(),
            display_name: "Control".into(),
            discovery_name: None,
            os: Os::Macos,
            daemon_version: "0.0.1".into(),
            secrets_file: dir.join("credentials.json"),
            audit_dir: dir.clone(),
            read_model_enabled: true,
        },
        Arc::new(|| Timestamp(1_790_900_000_000)),
    )
    .unwrap();
    let resolver = Arc::new(
        |_: String, _: u16| -> daemon::push::gateway::ResolveFuture {
            Box::pin(async { Ok(vec![IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))]) })
        },
    );
    let push = PushGateway::with_resolver(
        store,
        PushGatewaySettings {
            key_file: dir.join("push-vapid.json"),
            subject_origin: ORIGIN.into(),
            owner_uid: uid,
        },
        Arc::new(|| Timestamp(1_790_900_000_000)),
        resolver,
    );
    let auth = AuthManager::load(&dir.join("auth.json"), TOKEN.into()).unwrap();
    let machine = Machine {
        id: machine_id,
        name: "control".into(),
        os: Os::Macos,
        cpu: "cpu".into(),
        cpu_cores: 1,
        ram_bytes: 1,
        gpus: vec![],
        status: MachineStatus::Online,
        last_seen: Timestamp(1),
        role: None,
    };
    let state = AppState::new(sessions, machine, auth)
        .with_origins(OriginPolicy::new(&[ORIGIN.into()], false).unwrap())
        .with_control(Some(control))
        .with_push(Some(push));
    let peer: std::net::SocketAddr = "100.64.0.2:5000".parse().unwrap();
    Harness {
        app: router(state).layer(MockConnectInfo(peer)),
        dir,
    }
}

async fn call(
    app: &axum::Router,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(bearer) = bearer {
        request = request.header("authorization", format!("Bearer {bearer}"));
    }
    let request = if let Some(body) = body {
        request
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
    } else {
        request.body(Body::empty())
    }
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 128 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn owner(app: &axum::Router) -> String {
    let (_, body) = call(
        app,
        "POST",
        "/api/v1/auth/setup",
        Some(TOKEN),
        Some(json!({"password":"a-long-owner-password"})),
    )
    .await;
    body["token"].as_str().unwrap().into()
}

#[tokio::test]
async fn push_resources_require_owner_session_and_expose_only_public_material() {
    let h = harness();
    assert_eq!(
        call(&h.app, "GET", "/api/v1/push/public-key", None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&h.app, "GET", "/api/v1/push/public-key", Some(TOKEN), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let session = owner(&h.app).await;
    let (status, key) = call(
        &h.app,
        "GET",
        "/api/v1/push/public-key",
        Some(&session),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(key["subject_origin"], ORIGIN);
    assert!(key.get("private_key").is_none());
    assert_eq!(
        call(&h.app, "GET", "/api/v1/push/deliveries", None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, history) = call(
        &h.app,
        "GET",
        "/api/v1/push/deliveries",
        Some(&session),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(history, json!([]));
}

#[tokio::test]
async fn subscribe_list_enable_and_remove_are_bounded_and_secret_free() {
    let h = harness();
    let session = owner(&h.app).await;
    let secret = SecretKey::from_slice(&[7_u8; 32]).unwrap();
    let p256dh =
        Base64UrlUnpadded::encode_string(secret.public_key().to_encoded_point(false).as_bytes());
    let body = json!({
        "device_label":"Phone", "endpoint":"https://push.example.invalid/opaque",
        "expiration_time":null,
        "keys": {"p256dh":p256dh, "auth":"AQIDBAUGBwgJCgsMDQ4PEA"}
    });
    let (status, _) = call(
        &h.app,
        "POST",
        "/api/v1/push/subscriptions",
        Some(&session),
        Some(body),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, list) = call(
        &h.app,
        "GET",
        "/api/v1/push/subscriptions",
        Some(&session),
        None,
    )
    .await;
    let encoded = list.to_string();
    assert!(!encoded.contains("push.example.invalid"));
    assert!(!encoded.contains("p256dh"));
    let id = list["devices"][0]["subscription_id"].as_str().unwrap();
    let (enabled, _) = call(
        &h.app,
        "PUT",
        "/api/v1/push/provider",
        Some(&session),
        Some(json!({"mode":"WEB_PUSH"})),
    )
    .await;
    assert_eq!(enabled, StatusCode::OK);
    assert_eq!(
        call(
            &h.app,
            "DELETE",
            &format!("/api/v1/push/subscriptions/{id}"),
            Some(&session),
            None
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
}
