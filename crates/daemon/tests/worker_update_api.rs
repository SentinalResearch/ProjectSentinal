//! Updating one worker from the control (4.E §11) over loopback HTTP: the
//! worker's real API router, a code-paired link, and a fake `sentinal` CLI.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    api::{router, AppState},
    audit::AuditLog,
    auth::AuthManager,
    control::{routes::ControlRoutes, ControlService, ControlSettings, Detach},
    identity,
    link::{LinkManager, LinkSettings},
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
    worker_update::WorkerUpdater,
};
use protocol::{
    link::{LinkEndpoint, LinkFailureCode},
    worker_update::{WorkerUpdateRequest, WorkerUpdateState},
    LocalRequest, LocalResponse, Machine, MachineId, MachineStatus, Os, Secret, Timestamp,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use store::Store;

const CONTROL: &str = "01J9Y0C7RQ9M3X8V6T4N2P0SDB";
static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-wupd-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

async fn serve(router: axum::Router) -> (std::net::SocketAddr, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (address, task)
}

struct Fleet {
    control: ControlService,
    worker: LinkManager,
    worker_url: String,
    live: Arc<AtomicU32>,
    packages: PathBuf,
    worker_dir: PathBuf,
    tasks: Vec<tokio::task::JoinHandle<()>>,
    dirs: Vec<PathBuf>,
}

impl Drop for Fleet {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        for dir in &self.dirs {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

async fn fleet() -> Fleet {
    let control_dir = temp_dir("control");
    let worker_dir = temp_dir("worker");
    let packages = control_dir.join("packages");
    let clock = Arc::new(|| Timestamp(1_790_900_000_000));
    let control = ControlService::open(
        Arc::new(Store::open_in_memory().unwrap()),
        ControlSettings {
            public_url: "http://127.0.0.1:7421".into(),
            heartbeat_interval_ms: 30_000,
            development: true,
            machine_id: MachineId(CONTROL.into()),
            display_name: "Control".into(),
            discovery_name: None,
            package_dir: Some(packages.clone()),
            os: Os::Macos,
            daemon_version: "0.1.18".into(),
            secrets_file: control_dir.join("machine-credentials.json"),
            audit_dir: control_dir.clone(),
            read_model_enabled: true,
        },
        clock.clone(),
    )
    .unwrap();
    let auth = AuthManager::load(&control_dir.join("auth.json"), "control-token".into()).unwrap();
    let (control_address, control_task) =
        serve(ControlRoutes::new(Some(control.clone()), auth, false, true).router()).await;

    // The worker's listener first: its port is the endpoint the control calls.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let worker_address = listener.local_addr().unwrap();
    let identity_file = worker_dir.join("machine-id.json");
    let identity = identity::load_or_create(&identity_file, Timestamp(1), "pop-os")
        .map_err(|error| error.to_string());
    let worker = LinkManager::open(
        LinkSettings {
            link_file: worker_dir.join("link.json"),
            identity_file,
            identity,
            legacy_machine_id: "pop-os".into(),
            hostname: "pop-os".into(),
            endpoint: LinkEndpoint {
                address: "127.0.0.1".into(),
                port: worker_address.port(),
                dns_name: None,
            },
            os: Os::Linux,
            daemon_version: "0.1.18".into(),
            development: true,
            audit_dir: worker_dir.clone(),
            background: false,
        },
        clock.clone(),
    );
    let cli = worker_dir.join("sentinal");
    fs::write(
        &cli,
        "#!/bin/sh\necho \"$@\" > \"$(dirname \"$0\")/args.txt\"\nprintf '{\"ok\":false,\"reason\":\"signature check failed\"}' > \"$3\"\n",
    )
    .unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
    let live = Arc::new(AtomicU32::new(0));
    let count = live.clone();
    let updater = WorkerUpdater {
        data_dir: worker_dir.clone(),
        cli,
        detach: Detach::ProcessGroup,
        daemon_version: "0.1.18".into(),
        live_full_sessions: Arc::new(move || count.load(Ordering::SeqCst)),
        clock: clock.clone(),
    };
    let worker = worker.with_updater(updater.clone());
    let store = Arc::new(Store::open_in_memory().unwrap());
    let sessions = SessionManager::with_store(
        Arc::new(FakePty::new()),
        SessionManagerConfig {
            machine_id: worker.machine_id().unwrap(),
            data_dir: worker_dir.clone(),
            scrollback_lines: 10,
            clock: Arc::new(|| Timestamp(1)),
        },
        AuditLog::new(&worker_dir),
        store,
    )
    .unwrap();
    let machine = Machine {
        id: worker.machine_id().unwrap(),
        name: "pop-os".into(),
        os: Os::Linux,
        cpu: "c".into(),
        cpu_cores: 1,
        ram_bytes: 1,
        gpus: vec![],
        status: MachineStatus::Online,
        last_seen: Timestamp(1),
        role: None,
    };
    let worker_auth =
        AuthManager::load(&worker_dir.join("auth.json"), "worker-token".into()).unwrap();
    let state = AppState::new(sessions, machine, worker_auth)
        .with_link(worker.clone())
        .with_worker_updater(updater)
        .with_development(true);
    let app = router(state);
    let worker_task = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });

    let code = control.create_code().unwrap();
    let connected = worker
        .handle_local(LocalRequest::ConnectControl {
            control_url: format!("http://{control_address}"),
            pairing_code: Secret::new(code.code.expose()),
            display_name: None,
        })
        .await;
    assert!(
        matches!(connected, LocalResponse::Connection { .. }),
        "{connected:?}"
    );
    Fleet {
        control,
        worker,
        worker_url: format!("http://{worker_address}"),
        live,
        packages,
        worker_dir: worker_dir.clone(),
        tasks: vec![control_task, worker_task],
        dirs: vec![control_dir, worker_dir],
    }
}

fn publish(fleet: &Fleet, version: &str) {
    fs::create_dir_all(fleet.packages.join("channels")).unwrap();
    fs::write(
        fleet.packages.join("channels/stable.json"),
        format!("{{\"version\":\"{version}\"}}"),
    )
    .unwrap();
}

#[tokio::test]
async fn the_owner_updates_one_worker_and_sees_its_report() {
    let fleet = fleet().await;
    let worker_id = fleet.worker.machine_id().unwrap();
    let request = |allow_interrupt| WorkerUpdateRequest {
        version: "0.1.19".into(),
        allow_interrupt,
    };

    let unpublished = fleet
        .control
        .start_worker_update(&worker_id, &request(false))
        .await
        .unwrap_err();
    assert_eq!(unpublished.status, 409, "only the stable channel's release");
    publish(&fleet, "0.1.19");
    assert_eq!(
        fleet
            .control
            .registry()
            .unwrap()
            .available_release
            .as_deref(),
        Some("0.1.19")
    );

    fleet.live.store(2, Ordering::SeqCst);
    let refused = fleet
        .control
        .start_worker_update(&worker_id, &request(false))
        .await
        .unwrap_err();
    assert_eq!(refused.status, 409);
    assert_eq!(refused.failure.code, LinkFailureCode::LiveSessions);
    assert!(
        refused.failure.message.contains("2 live FULL Sessions"),
        "{}",
        refused.failure.message
    );

    let started = fleet
        .control
        .start_worker_update(&worker_id, &request(true))
        .await
        .unwrap();
    assert_eq!(started.state, WorkerUpdateState::Started);
    assert_eq!(started.previous, "0.1.18");

    let result = fleet.worker_dir.join("worker-update-result.json");
    for _ in 0..100 {
        if result.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let args = fs::read_to_string(fleet.worker_dir.join("args.txt")).unwrap();
    assert!(
        args.starts_with("update --result-file "),
        "the worker runs its own updater with no URL or command: {args}"
    );

    fleet.worker.heartbeat_once().await.unwrap();
    let registry = fleet.control.registry().unwrap();
    let entry = registry
        .machines
        .iter()
        .find(|machine| machine.machine_id == worker_id)
        .unwrap();
    let update = entry.update.as_ref().unwrap();
    assert_eq!(update.state, WorkerUpdateState::Failed);
    assert_eq!(update.reason.as_deref(), Some("signature check failed"));
}

#[tokio::test]
async fn the_worker_accepts_update_requests_only_from_its_control() {
    let fleet = fleet().await;
    let client = reqwest::Client::new();
    for bearer in [None, Some("worker-token"), Some(&*"a".repeat(64))] {
        let mut request = client
            .post(format!("{}/api/v1/link/update", fleet.worker_url))
            .json(&serde_json::json!({"version": "0.1.19", "allow_interrupt": true}));
        if let Some(bearer) = bearer {
            request = request.bearer_auth(bearer);
        }
        assert_eq!(
            request.send().await.unwrap().status(),
            reqwest::StatusCode::UNAUTHORIZED,
            "{bearer:?}"
        );
    }
    assert!(!fleet.worker_dir.join("worker-update.json").exists());
}
