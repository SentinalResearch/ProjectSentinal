//! `GET /api/v1/discovery` (4.E §5): unauthenticated, control-only, bounded,
//! and free of anything but the control's own name, ID, origin, and release.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::{to_bytes, Body},
    extract::connect_info::MockConnectInfo,
    http::{Request, StatusCode},
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
    pairing::{DiscoveryResponse, MAX_DISCOVERY_BYTES},
    Machine, MachineId, MachineStatus, Os, Timestamp,
};
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

const ORIGIN: &str = "https://sentinal-control.example.ts.net";
const CONTROL_ID: &str = "01J9Y0C7RQ9M3X8V6T4N2P0SDB";
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

fn daemon(control: bool, discovery_name: Option<&str>) -> Daemon {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-discovery-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let sessions = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: MachineId(CONTROL_ID.into()),
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
                machine_id: MachineId(CONTROL_ID.into()),
                display_name: "mac-mini".into(),
                discovery_name: discovery_name.map(Into::into),
                os: Os::Macos,
                daemon_version: "0.1.18".into(),
                secrets_file: dir.join("machine-credentials.json"),
                audit_dir: dir.clone(),
                read_model_enabled: true,
            },
            Arc::new(|| Timestamp(1_790_900_000_000)),
        )
        .unwrap()
    });
    let machine = Machine {
        id: MachineId(CONTROL_ID.into()),
        name: "mac-mini".into(),
        os: Os::Macos,
        cpu: "c".into(),
        cpu_cores: 1,
        ram_bytes: 1,
        gpus: vec![],
        status: MachineStatus::Online,
        last_seen: Timestamp(1),
        role: None,
    };
    let auth = AuthManager::load(&dir.join("auth.json"), "token".into()).unwrap();
    let state = AppState::new(sessions, machine, auth)
        .with_origins(OriginPolicy::new(&[ORIGIN.to_string()], false).unwrap())
        .with_control(control)
        .with_tailscale_serve(true);
    let peer: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    Daemon {
        app: router(state).layer(MockConnectInfo(peer)),
        dir,
    }
}

async fn get(daemon: &Daemon) -> (StatusCode, Option<String>, Vec<u8>) {
    let request = Request::get("/api/v1/discovery")
        .body(Body::empty())
        .unwrap();
    let response = daemon.app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let cache = response
        .headers()
        .get("cache-control")
        .map(|value| value.to_str().unwrap().to_string());
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, cache, bytes.to_vec())
}

#[tokio::test]
async fn a_control_names_itself_without_authentication() {
    let control = daemon(true, Some("Owner's Sentinal"));
    let (status, cache, body) = get(&control).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache.as_deref(), Some("no-store"));
    assert!(body.len() <= MAX_DISCOVERY_BYTES);
    let discovery: DiscoveryResponse = serde_json::from_slice(&body).unwrap();
    discovery.validate(ORIGIN).unwrap();
    assert_eq!(discovery.display_name, "Owner's Sentinal");
    assert_eq!(discovery.control_machine_id.0, CONTROL_ID);
    assert_eq!(discovery.release, "0.1.18");
    assert!(discovery.pairing.code && discovery.pairing.approval);

    let fallback = daemon(true, None);
    let (_, _, body) = get(&fallback).await;
    let discovery: DiscoveryResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(discovery.display_name, "mac-mini");
}

#[tokio::test]
async fn a_worker_is_not_discoverable() {
    let control = daemon(false, None);
    let (status, ..) = get(&control).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn discovery_is_rate_limited_control_wide() {
    let control = daemon(true, None);
    for _ in 0..60 {
        assert_eq!(get(&control).await.0, StatusCode::OK);
    }
    assert_eq!(get(&control).await.0, StatusCode::TOO_MANY_REQUESTS);
}
