//! Worker ↔ control pairing over real loopback HTTP (development mode).
//!
//! The control router and a worker `LinkManager` run in-process; background
//! heartbeats are off, so each test drives `heartbeat_once` deterministically.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    auth::AuthManager,
    control::{routes::ControlRoutes, ControlService, ControlSettings},
    identity,
    link::{LinkManager, LinkSettings},
};
use protocol::{
    link::{LinkEndpoint, LinkExchangeRequest, LinkMachineInfo, LocalLinkState, RegistryStatus},
    CaptureLevel, LocalErrorCode, LocalRequest, LocalResponse, MachineId, Os, ProjectedSession,
    ProjectionBatch, ProjectionChange, Secret, Session, SessionId, Timestamp,
    MAX_PROJECTION_BATCH_BYTES, PROJECTION_SCHEMA_VERSION,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicI64, AtomicU64, Ordering},
        Arc,
    },
};
use store::Store;

const CONTROL: &str = "01J9Y0CTRL0000000000000000";
const START: i64 = 1_790_900_000_000;
const DAY: i64 = 24 * 60 * 60 * 1_000;

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Harness {
    control: ControlService,
    url: String,
    clock: Arc<AtomicI64>,
    dir: PathBuf,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.server.abort();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-link-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

async fn serve(router: axum::Router) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (url, task)
}

async fn harness() -> Harness {
    let dir = temp_dir("control");
    let clock = Arc::new(AtomicI64::new(START));
    let tick = clock.clone();
    let control = ControlService::open(
        Arc::new(Store::open_in_memory().unwrap()),
        ControlSettings {
            public_url: "http://127.0.0.1:7421".into(),
            heartbeat_interval_ms: 30_000,
            development: true,
            machine_id: MachineId(CONTROL.into()),
            display_name: "Control".into(),
            os: Os::Macos,
            daemon_version: "0.0.1".into(),
            secrets_file: dir.join("machine-credentials.json"),
            audit_dir: dir.clone(),
            read_model_enabled: true,
        },
        Arc::new(move || Timestamp(tick.load(Ordering::SeqCst))),
    )
    .unwrap();
    let auth = AuthManager::load(&dir.join("auth.json"), "machine-token".into()).unwrap();
    let routes = ControlRoutes::new(Some(control.clone()), auth, false, false);
    let (url, server) = serve(routes.router()).await;
    Harness {
        control,
        url,
        clock,
        dir,
        server,
    }
}

struct Worker {
    manager: LinkManager,
    dir: PathBuf,
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn worker_with(h: &Harness, development: bool) -> Worker {
    let dir = temp_dir("worker");
    let identity_file = dir.join("machine-id.json");
    let identity = identity::load_or_create(&identity_file, Timestamp(START), "pop-os")
        .map_err(|error| error.to_string());
    let tick = h.clock.clone();
    let manager = LinkManager::open(
        LinkSettings {
            link_file: dir.join("link.json"),
            identity_file,
            identity,
            legacy_machine_id: "pop-os".into(),
            hostname: "pop-os".into(),
            endpoint: LinkEndpoint {
                address: "127.0.0.1".into(),
                port: 7431,
                dns_name: None,
            },
            os: Os::Linux,
            daemon_version: "0.0.1".into(),
            development,
            audit_dir: dir.clone(),
            background: false,
        },
        Arc::new(move || Timestamp(tick.load(Ordering::SeqCst))),
    );
    Worker { manager, dir }
}

fn worker(h: &Harness) -> Worker {
    worker_with(h, true)
}

impl Worker {
    async fn connect(&self, url: &str, code: &str, name: Option<&str>) -> LocalResponse {
        self.manager
            .handle_local(LocalRequest::ConnectControl {
                control_url: url.into(),
                pairing_code: Secret::new(code),
                display_name: name.map(Into::into),
            })
            .await
    }

    async fn status(&self) -> protocol::link::ConnectionReport {
        match self
            .manager
            .handle_local(LocalRequest::ConnectionStatus)
            .await
        {
            LocalResponse::Connection { connection } => connection,
            other => panic!("unexpected {other:?}"),
        }
    }

    fn machine_id(&self) -> MachineId {
        self.manager.machine_id().unwrap()
    }

    fn link_file(&self) -> String {
        fs::read_to_string(self.dir.join("link.json")).unwrap_or_default()
    }
}

fn error_code(response: &LocalResponse) -> LocalErrorCode {
    match response {
        LocalResponse::Error { code, .. } => *code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn registry_status(h: &Harness, id: &MachineId) -> RegistryStatus {
    h.control
        .registry()
        .unwrap()
        .machines
        .into_iter()
        .find(|machine| &machine.machine_id == id)
        .unwrap()
        .status
}

#[tokio::test]
async fn connect_pairs_the_worker_and_stores_owner_only_credentials() {
    let h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();

    let response = w.connect(&h.url, code.code.expose(), Some("RTX PC")).await;

    let LocalResponse::Connection { connection } = response else {
        panic!("{response:?}")
    };
    assert_eq!(connection.state, LocalLinkState::Active);
    assert_eq!(connection.display_name.as_deref(), Some("RTX PC"));
    assert_eq!(
        connection.control_machine_id,
        Some(MachineId(CONTROL.into()))
    );
    let mode = fs::metadata(w.dir.join("link.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    assert!(!w.link_file().contains(code.code.expose()));
    let presented = h.control.presented_credential(&w.machine_id()).unwrap();
    assert!(
        !w.link_file().contains(presented.expose()),
        "worker keeps only a hash"
    );
    let identity = w
        .manager
        .verify_control_credential(presented.expose())
        .unwrap();
    assert_eq!(identity.machine_id, w.machine_id());
    assert!(w
        .manager
        .verify_control_credential(&"0".repeat(64))
        .is_none());
    h.clock.fetch_add(90 * DAY, Ordering::SeqCst);
    assert!(
        w.manager
            .verify_control_credential(presented.expose())
            .is_none(),
        "an expired control credential cannot authorize worker APIs"
    );
}

#[tokio::test]
async fn the_first_heartbeat_marks_the_worker_online() {
    let h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();
    w.connect(&h.url, code.code.expose(), None).await;

    w.manager.heartbeat_once().await.unwrap();

    assert_eq!(registry_status(&h, &w.machine_id()), RegistryStatus::Online);
    assert_eq!(
        w.status().await.display_name.as_deref(),
        Some("pop-os"),
        "hostname default"
    );
}

#[tokio::test]
async fn projection_route_accepts_the_advertised_body_limit() {
    let h = harness().await;
    let code = h.control.create_code().unwrap();
    let machine_id = MachineId("01J9Z3K8V6Q2T7M4X5C0RNB8DA".into());
    let exchange = h
        .control
        .exchange(
            LinkExchangeRequest {
                pairing_code: code.code,
                machine: LinkMachineInfo {
                    machine_id: machine_id.clone(),
                    display_name: "Large projection worker".into(),
                    os: Os::Linux,
                    daemon_version: "0.4.1".into(),
                    capabilities: vec![],
                },
                endpoint: LinkEndpoint {
                    address: "100.74.239.89".into(),
                    port: 7420,
                    dns_name: None,
                },
                control_credential: Secret::new("a".repeat(64)),
            },
            "tester",
        )
        .unwrap();
    let changes = (1_u64..=128)
        .map(|cursor| {
            let mut session = Session::new(
                SessionId(cursor),
                machine_id.clone(),
                "/bin/zsh",
                "/tmp",
                Timestamp(START),
            );
            session.capture = CaptureLevel::Full;
            session.title = Some("🚀".repeat(48));
            ProjectionChange::SessionUpsert {
                cursor,
                session: ProjectedSession::from_session(&session),
            }
        })
        .collect();
    let batch = ProjectionBatch::Delta {
        schema_version: PROJECTION_SCHEMA_VERSION,
        machine_id,
        source_epoch: "pe_large_body".into(),
        base_cursor: 0,
        through_cursor: 128,
        sent_at: Timestamp(START),
        changes,
    };
    let encoded = serde_json::to_vec(&batch).unwrap();
    assert!(
        encoded.len() > 64 * 1024,
        "fixture must exceed link default"
    );
    assert!(encoded.len() <= MAX_PROJECTION_BATCH_BYTES);

    let response = reqwest::Client::new()
        .post(format!("{}/api/v1/link/projection", h.url))
        .bearer_auth(exchange.worker_credential.expose())
        .body(encoded)
        .header("content-type", "application/json")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn owner_renames_reach_the_worker_through_heartbeats() {
    let h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();
    w.connect(&h.url, code.code.expose(), Some("RTX PC")).await;
    h.control.rename(&w.machine_id(), "RTX 4090 box").unwrap();

    w.manager.heartbeat_once().await.unwrap();

    assert_eq!(
        w.status().await.display_name.as_deref(),
        Some("RTX 4090 box")
    );
}

#[tokio::test]
async fn a_wrong_code_is_rejected_and_writes_nothing() {
    let h = harness().await;
    let w = worker(&h);
    h.control.create_code().unwrap();

    let response = w.connect(&h.url, "00000-00000", None).await;

    assert_eq!(error_code(&response), LocalErrorCode::CodeRejected);
    assert!(!w.dir.join("link.json").exists());
    assert_eq!(w.status().await.state, LocalLinkState::NotConnected);
}

#[tokio::test]
async fn a_code_that_expires_during_the_exchange_is_rejected() {
    let h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();
    h.clock.fetch_add(10 * 60 * 1_000, Ordering::SeqCst);

    let response = w.connect(&h.url, code.code.expose(), None).await;

    assert_eq!(error_code(&response), LocalErrorCode::CodeRejected);
}

#[tokio::test]
async fn an_already_connected_worker_refuses_a_second_connect() {
    let h = harness().await;
    let w = worker(&h);
    let first = h.control.create_code().unwrap();
    let second = h.control.create_code().unwrap();
    w.connect(&h.url, first.code.expose(), None).await;

    let response = w.connect(&h.url, second.code.expose(), None).await;

    assert_eq!(error_code(&response), LocalErrorCode::AlreadyConnected);
    let status = h.control.code_status(&second.code_id).unwrap();
    assert_eq!(
        status.state,
        protocol::link::PairingCodeState::Waiting,
        "code not spent"
    );
}

#[tokio::test]
async fn an_unreachable_control_is_reported_without_writing_a_link() {
    let h = harness().await;
    let w = worker(&h);
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };

    let response = w
        .connect(&format!("http://{closed}"), "7KQ4M-2XZ9C", None)
        .await;

    assert_eq!(error_code(&response), LocalErrorCode::ControlUnreachable);
    assert!(!w.dir.join("link.json").exists());
}

#[tokio::test]
async fn a_daemon_that_is_not_a_control_is_reported() {
    let dir = temp_dir("plain");
    let auth = AuthManager::load(&dir.join("auth.json"), "t".into()).unwrap();
    let (url, server) = serve(ControlRoutes::new(None, auth, false, false).router()).await;
    let h = harness().await;
    let w = worker(&h);

    let response = w.connect(&url, "7KQ4M-2XZ9C", None).await;

    assert_eq!(error_code(&response), LocalErrorCode::ControlUnreachable);
    let LocalResponse::Error { message, .. } = response else {
        unreachable!()
    };
    assert!(message.contains("not a control"), "{message}");
    server.abort();
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn production_workers_refuse_plain_http_controls() {
    let h = harness().await;
    let w = worker_with(&h, false);
    let response = w.connect(&h.url, "7KQ4M-2XZ9C", None).await;
    assert_eq!(error_code(&response), LocalErrorCode::InvalidRequest);
}

#[tokio::test]
async fn revocation_reaches_the_worker_and_a_new_code_reconnects_the_same_machine() {
    let h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();
    w.connect(&h.url, code.code.expose(), Some("RTX PC")).await;
    h.control.rename(&w.machine_id(), "Owner name").unwrap();
    h.control.revoke(&w.machine_id()).unwrap();

    assert!(w.manager.heartbeat_once().await.is_err());

    let status = w.status().await;
    assert_eq!(status.state, LocalLinkState::Revoked);
    assert!(
        !w.link_file().contains("worker_credential\":\""),
        "credential deleted"
    );
    let again = h.control.create_code().unwrap();
    let LocalResponse::Connection { connection } =
        w.connect(&h.url, again.code.expose(), None).await
    else {
        panic!("reconnect failed")
    };
    assert_eq!(connection.state, LocalLinkState::Active);
    assert_eq!(connection.machine_id, w.machine_id());
    assert_eq!(connection.display_name.as_deref(), Some("Owner name"));
}

#[tokio::test]
async fn disconnect_tells_the_control_and_removes_the_link() {
    let h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();
    w.connect(&h.url, code.code.expose(), None).await;

    let response = w
        .manager
        .handle_local(LocalRequest::DisconnectControl)
        .await;

    let LocalResponse::Connection { connection } = response else {
        panic!("{response:?}")
    };
    assert_eq!(connection.state, LocalLinkState::NotConnected);
    assert_eq!(connection.message, None);
    assert!(!w.dir.join("link.json").exists());
    assert_eq!(
        registry_status(&h, &w.machine_id()),
        RegistryStatus::Disconnected
    );
    assert_eq!(
        error_code(
            &w.manager
                .handle_local(LocalRequest::DisconnectControl)
                .await
        ),
        LocalErrorCode::NotConnected
    );
}

#[tokio::test]
async fn disconnect_with_an_unreachable_control_still_removes_the_local_link() {
    let mut h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();
    w.connect(&h.url, code.code.expose(), None).await;
    h.server.abort();
    let _ = (&mut h.server).await;

    let response = w
        .manager
        .handle_local(LocalRequest::DisconnectControl)
        .await;

    let LocalResponse::Connection { connection } = response else {
        panic!("{response:?}")
    };
    assert!(
        connection.message.unwrap().contains("revoke"),
        "owner is told to revoke"
    );
    assert!(!w.dir.join("link.json").exists());
}

#[tokio::test]
async fn three_failed_heartbeats_report_the_control_unreachable() {
    let mut h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();
    w.connect(&h.url, code.code.expose(), None).await;
    h.server.abort();
    let _ = (&mut h.server).await;

    for _ in 0..2 {
        assert!(w.manager.heartbeat_once().await.is_err());
        assert_eq!(w.status().await.state, LocalLinkState::Active);
    }
    assert!(w.manager.heartbeat_once().await.is_err());
    assert_eq!(w.status().await.state, LocalLinkState::ControlUnreachable);
}

#[tokio::test]
async fn credentials_refresh_before_they_expire() {
    let h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();
    w.connect(&h.url, code.code.expose(), None).await;
    let before = h.control.presented_credential(&w.machine_id()).unwrap();
    let file_before = w.link_file();
    h.clock.fetch_add(61 * DAY, Ordering::SeqCst);

    w.manager.heartbeat_once().await.unwrap();

    let after = h.control.presented_credential(&w.machine_id()).unwrap();
    assert_ne!(before, after);
    assert_ne!(file_before, w.link_file());
    assert!(w
        .manager
        .verify_control_credential(after.expose())
        .is_some());
    assert!(w
        .manager
        .verify_control_credential(before.expose())
        .is_none());
    w.manager.heartbeat_once().await.unwrap();
}

#[tokio::test]
async fn a_corrupt_identity_blocks_linking_but_reports_why() {
    let h = harness().await;
    let w = worker(&h);
    let dir = temp_dir("corrupt");
    let identity_file = dir.join("machine-id.json");
    fs::write(&identity_file, "{broken").unwrap();
    let corrupt = LinkManager::open(
        LinkSettings {
            link_file: dir.join("link.json"),
            identity_file: identity_file.clone(),
            identity: Err("machine-id.json is invalid".into()),
            legacy_machine_id: "pop-os".into(),
            hostname: "pop-os".into(),
            endpoint: LinkEndpoint {
                address: "127.0.0.1".into(),
                port: 7431,
                dns_name: None,
            },
            os: Os::Linux,
            daemon_version: "0.0.1".into(),
            development: true,
            audit_dir: dir.clone(),
            background: false,
        },
        Arc::new(|| Timestamp(START)),
    );
    let code = h.control.create_code().unwrap();
    let response = corrupt
        .handle_local(LocalRequest::ConnectControl {
            control_url: h.url.clone(),
            pairing_code: Secret::new(code.code.expose()),
            display_name: None,
        })
        .await;
    assert_eq!(error_code(&response), LocalErrorCode::IdentityCorrupt);
    assert_eq!(
        error_code(&corrupt.handle_local(LocalRequest::GetIdentity).await),
        LocalErrorCode::IdentityCorrupt
    );

    let reset = corrupt.handle_local(LocalRequest::ResetIdentity).await;
    let LocalResponse::Identity { machine_id, .. } = reset else {
        panic!("{reset:?}")
    };
    assert!(machine_id.is_ulid());
    assert_ne!(machine_id, w.machine_id());
    let _ = fs::remove_dir_all(dir);
}

#[tokio::test]
async fn worker_audit_and_link_file_never_contain_the_code() {
    let h = harness().await;
    let w = worker(&h);
    let code = h.control.create_code().unwrap();
    w.connect(&h.url, "00000-00000", None).await;
    w.connect(&h.url, code.code.expose(), None).await;
    let audit = fs::read_to_string(w.dir.join("link-audit.jsonl")).unwrap();
    for secret in [code.code.expose(), "00000-00000"] {
        assert!(!audit.contains(secret));
        assert!(!w.link_file().contains(secret));
    }
    assert!(audit.contains("CONNECTED") && audit.contains("CONNECT_FAILED"));
}
