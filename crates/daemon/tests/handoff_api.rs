//! A control stepping down hands its worker to another control, over loopback
//! HTTP: old control → worker `/link/move` → approval request at the new
//! control → owner approves → the worker's link points at the new control.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    api::{router, AppState},
    audit::AuditLog,
    auth::AuthManager,
    control::{
        routes::ControlRoutes, ControlService, ControlSettings, PairingVerifier, VerifyFuture,
    },
    identity,
    link::{LinkManager, LinkSettings},
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{
    link::{LinkEndpoint, LocalLinkState},
    pairing::{
        LinkMoveRequest, PairingApproval, PairingProof, PairingRequestState, TailscaleIdentity,
    },
    LocalRequest, LocalResponse, Machine, MachineId, MachineStatus, Os, Secret, Timestamp,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
    time::Duration,
};
use store::Store;

const OLD: &str = "01J9Y0C7RQ9M3X8V6T4N2P0SDB";
const NEW: &str = "01K2C7RQ9M3X8V6T4N2P0SDB5A";
static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-handoff-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

struct Callback(Arc<OnceLock<LinkManager>>);

impl PairingVerifier for Callback {
    fn proof<'a>(
        &'a self,
        _endpoint: &'a LinkEndpoint,
        request_id: &'a str,
    ) -> VerifyFuture<'a, PairingProof> {
        Box::pin(async move {
            self.0
                .get()
                .and_then(|worker| worker.pairing_proof(request_id))
                .ok_or_else(|| "404".to_string())
        })
    }
    fn whois<'a>(&'a self, address: &'a str) -> VerifyFuture<'a, TailscaleIdentity> {
        Box::pin(async move {
            Ok(TailscaleIdentity {
                node: "rtx".into(),
                login: "owner@example.com".into(),
                os: "linux".into(),
                address: address.into(),
            })
        })
    }
}

async fn control(
    id: &str,
    dir: &std::path::Path,
    verifier: Option<Arc<dyn PairingVerifier>>,
) -> (ControlService, String, tokio::task::JoinHandle<()>) {
    let mut service = ControlService::open(
        Arc::new(Store::open_in_memory().unwrap()),
        ControlSettings {
            public_url: "http://127.0.0.1:7421".into(),
            heartbeat_interval_ms: 30_000,
            development: true,
            machine_id: MachineId(id.into()),
            display_name: "control".into(),
            discovery_name: None,
            package_dir: None,
            os: Os::Linux,
            daemon_version: "0.2.2".into(),
            secrets_file: dir.join("machine-credentials.json"),
            audit_dir: dir.to_path_buf(),
            read_model_enabled: true,
        },
        Arc::new(|| Timestamp(1_790_900_000_000)),
    )
    .unwrap();
    if let Some(verifier) = verifier {
        service = service.with_pairing_verifier(verifier);
    }
    let auth = AuthManager::load(&dir.join("auth.json"), "token".into()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = ControlRoutes::new(Some(service.clone()), auth, false, true).router();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (service, url, task)
}

#[tokio::test]
async fn a_stepping_down_control_hands_its_worker_to_the_new_one() {
    let (old_dir, new_dir, worker_dir) = (temp_dir("old"), temp_dir("new"), temp_dir("worker"));
    let slot = Arc::new(OnceLock::new());
    let (old, old_url, old_task) = control(OLD, &old_dir, None).await;
    let (new, new_url, new_task) =
        control(NEW, &new_dir, Some(Arc::new(Callback(slot.clone())))).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let identity_file = worker_dir.join("machine-id.json");
    let identity = identity::load_or_create(&identity_file, Timestamp(1), "rtx")
        .map_err(|error| error.to_string());
    let worker = LinkManager::open(
        LinkSettings {
            link_file: worker_dir.join("link.json"),
            identity_file,
            identity,
            legacy_machine_id: "rtx".into(),
            hostname: "rtx".into(),
            endpoint: LinkEndpoint {
                address: "127.0.0.1".into(),
                port,
                dns_name: None,
            },
            os: Os::Linux,
            daemon_version: "0.2.2".into(),
            development: true,
            audit_dir: worker_dir.clone(),
            background: false,
        },
        Arc::new(|| Timestamp(1_790_900_000_000)),
    );
    slot.set(worker.clone()).ok();
    let sessions = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: worker.machine_id().unwrap(),
            data_dir: worker_dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(1)),
        },
        AuditLog::new(&worker_dir),
        Arc::new(Store::open_in_memory().unwrap()),
    )
    .unwrap();
    let machine = Machine {
        id: worker.machine_id().unwrap(),
        name: "rtx".into(),
        os: Os::Linux,
        cpu: "c".into(),
        cpu_cores: 1,
        ram_bytes: 1,
        gpus: vec![],
        status: MachineStatus::Online,
        last_seen: Timestamp(1),
        role: None,
    };
    let auth = AuthManager::load(&worker_dir.join("auth.json"), "worker-token".into()).unwrap();
    let app = router(
        AppState::new(sessions, machine, auth)
            .with_link(worker.clone())
            .with_development(true),
    );
    let worker_task = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    // Paired to the old control with a code, and renamed there by the owner.
    let code = old.create_code().unwrap();
    let connected = worker
        .handle_local(LocalRequest::ConnectControl {
            control_url: old_url.clone(),
            pairing_code: Secret::new(code.code.expose()),
            display_name: Some("RTX PC".into()),
        })
        .await;
    assert!(
        matches!(connected, LocalResponse::Connection { .. }),
        "{connected:?}"
    );

    let target = LinkMoveRequest {
        control_url: new_url.clone(),
        control_machine_id: MachineId(NEW.into()),
    };
    let listed = old.handoff_workers(&target, true).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert!(!listed[0].moved, "a dry run moves nothing");
    let refused = old
        .handoff_workers(
            &LinkMoveRequest {
                control_url: new_url.clone(),
                control_machine_id: MachineId(OLD.into()),
            },
            false,
        )
        .await;
    assert!(refused.is_err(), "a control cannot hand workers to itself");

    let moved = old.handoff_workers(&target, false).await.unwrap();
    assert!(moved[0].moved, "{:?}", moved[0].message);

    let mut request_id = None;
    for _ in 0..100 {
        if let Some(request) = new
            .pairing_requests()
            .unwrap()
            .requests
            .into_iter()
            .find(|request| request.verified)
        {
            assert_eq!(
                request.claimed.display_name, "RTX PC",
                "the owner's name moves with the worker"
            );
            request_id = Some(request.request_id);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let request_id = request_id.expect("the worker asked the new control");
    // Still linked to the old control until the owner approves.
    let LocalResponse::Connection { connection } =
        worker.handle_local(LocalRequest::ConnectionStatus).await
    else {
        panic!()
    };
    assert_eq!(connection.control_url.as_deref(), Some(old_url.as_str()));

    new.approve_pairing_request(&request_id, &PairingApproval { display_name: None })
        .unwrap();
    let mut switched = false;
    for _ in 0..80 {
        let LocalResponse::Connection { connection } =
            worker.handle_local(LocalRequest::ConnectionStatus).await
        else {
            panic!()
        };
        if connection.control_url.as_deref() == Some(new_url.as_str()) {
            assert_eq!(connection.state, LocalLinkState::Active);
            assert_eq!(connection.control_machine_id, Some(MachineId(NEW.into())));
            switched = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        switched,
        "the worker moved to the new control after approval"
    );
    assert_eq!(
        new.pairing_requests().unwrap().requests[0].state,
        PairingRequestState::Claimed
    );

    for task in [old_task, new_task, worker_task] {
        task.abort();
    }
    for dir in [old_dir, new_dir, worker_dir] {
        let _ = fs::remove_dir_all(dir);
    }
}

#[tokio::test]
async fn only_the_current_control_can_move_a_worker() {
    let worker_dir = temp_dir("auth");
    let identity_file = worker_dir.join("machine-id.json");
    let identity = identity::load_or_create(&identity_file, Timestamp(1), "rtx")
        .map_err(|error| error.to_string());
    let worker = LinkManager::open(
        LinkSettings {
            link_file: worker_dir.join("link.json"),
            identity_file,
            identity,
            legacy_machine_id: "rtx".into(),
            hostname: "rtx".into(),
            endpoint: LinkEndpoint {
                address: "127.0.0.1".into(),
                port: 1,
                dns_name: None,
            },
            os: Os::Linux,
            daemon_version: "0.2.2".into(),
            development: true,
            audit_dir: worker_dir.clone(),
            background: false,
        },
        Arc::new(|| Timestamp(1)),
    );
    let sessions = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: worker.machine_id().unwrap(),
            data_dir: worker_dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(1)),
        },
        AuditLog::new(&worker_dir),
        Arc::new(Store::open_in_memory().unwrap()),
    )
    .unwrap();
    let machine = Machine {
        id: worker.machine_id().unwrap(),
        name: "rtx".into(),
        os: Os::Linux,
        cpu: "c".into(),
        cpu_cores: 1,
        ram_bytes: 1,
        gpus: vec![],
        status: MachineStatus::Online,
        last_seen: Timestamp(1),
        role: None,
    };
    let auth = AuthManager::load(&worker_dir.join("auth.json"), "worker-token".into()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/api/v1/link/move", listener.local_addr().unwrap());
    let app = router(
        AppState::new(sessions, machine, auth)
            .with_link(worker)
            .with_development(true),
    );
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let body = serde_json::json!({"control_url": "https://attacker.example.ts.net", "control_machine_id": NEW});
    for bearer in [None, Some("worker-token".to_string()), Some("0".repeat(64))] {
        let mut request = reqwest::Client::new().post(&url).json(&body);
        if let Some(bearer) = bearer.as_deref() {
            request = request.bearer_auth(bearer);
        }
        assert_eq!(
            request.send().await.unwrap().status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "{bearer:?}"
        );
    }
    task.abort();
    let _ = fs::remove_dir_all(worker_dir);
}
