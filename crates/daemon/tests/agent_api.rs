//! Milestone 6.0 agent tokens: only the owner creates, lists and revokes
//! them; an agent token reads the structured read model and nothing else.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::{to_bytes, Body},
    extract::connect_info::MockConnectInfo,
    http::{Method, Request, StatusCode},
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
    agent::{AgentIdentity, AgentList, AgentUpdates, CreatedAgent},
    link::{LinkEndpoint, LinkExchangeRequest, LinkMachineInfo, MachineCapability},
    CaptureLevel, ControlProjectionSnapshot, Machine, MachineId, MachineStatus, Os,
    ProjectedSession, ProjectionBatch, ProjectionChange, Secret, Session, SessionId, Timestamp,
    PROJECTION_SCHEMA_VERSION,
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
    time::{Duration, Instant},
};
use store::Store;
use tower::ServiceExt;

const ORIGIN: &str = "https://sentinal-control.example.ts.net";
const CONTROL_ID: &str = "01J9Y0C7RQ9M3X8V6T4N2P0SDB";
const MACHINE_TOKEN: &str = "bootstrap-machine-token";
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Daemon {
    app: Router,
    dir: PathBuf,
    owner: String,
    control: ControlService,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

async fn daemon() -> Daemon {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-agents-api-{}-{}",
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
    let control = ControlService::open(
        store,
        ControlSettings {
            public_url: ORIGIN.into(),
            heartbeat_interval_ms: 30_000,
            development: false,
            machine_id: MachineId(CONTROL_ID.into()),
            display_name: "mac-mini".into(),
            discovery_name: None,
            package_dir: None,
            os: Os::Macos,
            daemon_version: "0.3.0".into(),
            secrets_file: dir.join("machine-credentials.json"),
            audit_dir: dir.clone(),
            read_model_enabled: true,
        },
        Arc::new(|| Timestamp(1_790_900_000_000)),
    )
    .unwrap();
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
    let auth = AuthManager::load(&dir.join("auth.json"), MACHINE_TOKEN.into()).unwrap();
    let owner = auth
        .setup(MACHINE_TOKEN, "correct horse battery staple")
        .await
        .unwrap()
        .token;
    let state = AppState::new(sessions, machine, auth)
        .with_origins(OriginPolicy::new(&[ORIGIN.to_string()], false).unwrap())
        .with_control(Some(control.clone()));
    let peer: SocketAddr = "100.64.0.2:50000".parse().unwrap();
    Daemon {
        app: router(state).layer(MockConnectInfo(peer)),
        dir,
        owner,
        control,
    }
}

const WORKER: &str = "01J9Z3K8V6Q2T7M4X5C0RNB8DA";

/// Pair a worker and upload one Session to the read model, as a worker link does.
fn worker_reports_a_session(control: &ControlService) {
    let code = control.create_code().unwrap();
    let credential = control
        .exchange(
            LinkExchangeRequest {
                pairing_code: Secret::new(code.code.expose()),
                machine: LinkMachineInfo {
                    machine_id: MachineId(WORKER.into()),
                    display_name: "pop-os".into(),
                    os: Os::Linux,
                    daemon_version: "0.3.0".into(),
                    capabilities: vec![MachineCapability::Sessions],
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
        .unwrap()
        .worker_credential;
    let mut session = Session::new(
        SessionId(7),
        MachineId(WORKER.into()),
        "/bin/zsh",
        "/home/u",
        Timestamp(1),
    );
    session.title = Some("Training".into());
    session.capture = CaptureLevel::Full;
    control
        .accept_projection(
            credential.expose(),
            ProjectionBatch::Delta {
                schema_version: PROJECTION_SCHEMA_VERSION,
                machine_id: MachineId(WORKER.into()),
                source_epoch: "pe_agent".into(),
                base_cursor: 0,
                through_cursor: 1,
                sent_at: Timestamp(1),
                changes: vec![ProjectionChange::SessionUpsert {
                    cursor: 1,
                    session: ProjectedSession::from_session(&session),
                }],
            },
        )
        .unwrap();
}

impl Daemon {
    async fn call(
        &self,
        method: Method,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Vec<u8>) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
        }
        let request = match body {
            Some(body) => request
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
            None => request.body(Body::empty()).unwrap(),
        };
        let response = self.app.clone().oneshot(request).await.unwrap();
        let status = response.status();
        (
            status,
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .to_vec(),
        )
    }

    async fn create(&self, name: &str, level: &str) -> CreatedAgent {
        let (status, body) = self
            .call(
                Method::POST,
                "/api/v1/agents",
                Some(&self.owner),
                Some(json!({"name": name, "level": level})),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::CREATED,
            "{}",
            String::from_utf8_lossy(&body)
        );
        serde_json::from_slice(&body).unwrap()
    }

    fn audit(&self) -> String {
        fs::read_to_string(self.dir.join("pairing-audit.jsonl")).unwrap_or_default()
    }
}

#[tokio::test]
async fn the_owner_creates_lists_and_revokes_agents_and_the_token_is_shown_once() {
    let daemon = daemon().await;
    let created = daemon.create("claude", "SUGGEST").await;
    assert!(created.token.expose().starts_with("sat_agt_"));

    let (status, body) = daemon
        .call(Method::GET, "/api/v1/agents", Some(&daemon.owner), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!String::from_utf8_lossy(&body).contains(created.token.expose()));
    let list: AgentList = serde_json::from_slice(&body).unwrap();
    assert_eq!(list.agents, vec![created.agent.clone()]);

    let path = format!("/api/v1/agents/{}", created.agent.id);
    assert_eq!(
        daemon
            .call(Method::DELETE, &path, Some(&daemon.owner), None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        daemon
            .call(Method::DELETE, &path, Some(&daemon.owner), None)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (status, _) = daemon
        .call(
            Method::GET,
            "/api/v1/agent/whoami",
            Some(created.token.expose()),
            None,
        )
        .await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "a revoked token stops at once"
    );

    let audit = daemon.audit();
    assert!(audit.contains("AGENT_CREATED") && audit.contains("AGENT_REVOKED"));
    assert!(audit.contains(&created.agent.id));
    assert!(
        !audit.contains(created.token.expose()),
        "tokens never reach the audit"
    );
}

#[tokio::test]
async fn only_an_owner_browser_session_manages_agents() {
    let daemon = daemon().await;
    let agent = daemon.create("claude", "SUGGEST").await;
    let body = json!({"name": "sneaky", "level": "SUGGEST"});
    for bearer in [
        None,
        Some(MACHINE_TOKEN),
        Some(agent.token.expose()),
        Some("sat_agt_0000000000000000_00"),
    ] {
        let (status, _) = daemon
            .call(Method::POST, "/api/v1/agents", bearer, Some(body.clone()))
            .await;
        assert!(
            matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
            "{bearer:?}: {status}"
        );
        let (status, _) = daemon
            .call(Method::GET, "/api/v1/agents", bearer, None)
            .await;
        assert!(
            matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
            "{bearer:?}: {status}"
        );
    }
}

#[tokio::test]
async fn level_three_and_bad_names_cannot_be_created() {
    let daemon = daemon().await;
    for body in [
        json!({"name": "claude", "level": "ACT"}),
        json!({"name": "claude", "level": 3}),
        json!({"name": "rm -rf ~", "level": "OBSERVE"}),
    ] {
        let (status, _) = daemon
            .call(
                Method::POST,
                "/api/v1/agents",
                Some(&daemon.owner),
                Some(body.clone()),
            )
            .await;
        assert!(status.is_client_error(), "{body}: {status}");
    }
}

#[tokio::test]
async fn an_agent_reads_the_read_model_and_knows_who_it_is() {
    let daemon = daemon().await;
    let agent = daemon.create("codex", "OBSERVE").await;
    let token = agent.token.expose();

    let (status, body) = daemon
        .call(Method::GET, "/api/v1/agent/whoami", Some(token), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let identity: AgentIdentity = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        (identity.name.as_str(), identity.level.number()),
        ("codex", 0)
    );

    let (status, body) = daemon
        .call(Method::GET, "/api/v1/agent/snapshot", Some(token), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let snapshot: ControlProjectionSnapshot = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        snapshot.control_machine_id,
        Some(MachineId(CONTROL_ID.into()))
    );
}

#[tokio::test]
async fn agent_routes_accept_no_other_credential() {
    let daemon = daemon().await;
    for bearer in [None, Some(MACHINE_TOKEN), Some(daemon.owner.as_str())] {
        for path in [
            "/api/v1/agent/whoami",
            "/api/v1/agent/snapshot",
            "/api/v1/agent/updates",
        ] {
            let (status, _) = daemon.call(Method::GET, path, bearer, None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{path} with {bearer:?}");
        }
    }
}

#[tokio::test]
async fn an_agent_token_opens_no_terminal_owner_or_worker_route() {
    let daemon = daemon().await;
    let agent = daemon.create("claude", "SUGGEST").await;
    let token = agent.token.expose();
    for (method, path) in [
        (Method::GET, "/api/v1/sessions"),
        (Method::GET, "/api/v1/machine"),
        (Method::GET, "/api/v1/ws"),
        (Method::GET, "/api/v1/sessions/1/output"),
        (Method::GET, "/api/v1/machines"),
        (
            Method::GET,
            "/api/v1/machines/01J9Z3K8V6Q2T7M4X5C0RNB8DA/ws",
        ),
        (
            Method::GET,
            "/api/v1/machines/01J9Z3K8V6Q2T7M4X5C0RNB8DA/sessions/1/output",
        ),
        (Method::GET, "/api/v1/control/snapshot"),
        (Method::POST, "/api/v1/pairing/codes"),
    ] {
        let (status, _) = daemon.call(method.clone(), path, Some(token), None).await;
        assert!(
            matches!(
                status,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::BAD_REQUEST
            ),
            "{method} {path}: {status}"
        );
    }
}

#[tokio::test]
async fn updates_wait_for_a_change_and_end_quietly_when_none_comes() {
    let daemon = daemon().await;
    let agent = daemon.create("claude", "OBSERVE").await;
    let token = agent.token.expose();
    let (_, body) = daemon
        .call(Method::GET, "/api/v1/agent/snapshot", Some(token), None)
        .await;
    let revision = serde_json::from_slice::<ControlProjectionSnapshot>(&body)
        .unwrap()
        .view_revision;

    let path = format!("/api/v1/agent/updates?after={revision}");
    let (status, body) = daemon.call(Method::GET, &path, Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(serde_json::from_slice::<AgentUpdates>(&body)
        .unwrap()
        .messages
        .is_empty());

    let started = Instant::now();
    let (status, body) = daemon
        .call(
            Method::GET,
            &format!("{path}&wait_ms=60"),
            Some(token),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(serde_json::from_slice::<AgentUpdates>(&body)
        .unwrap()
        .messages
        .is_empty());
    assert!(started.elapsed() >= Duration::from_millis(50), "it waited");
    assert!(started.elapsed() < Duration::from_secs(5));

    let (_, body) = daemon
        .call(
            Method::GET,
            &format!("/api/v1/agent/updates?after={}", revision + 1000),
            Some(token),
            None,
        )
        .await;
    let updates: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        updates["messages"][0]["type"], "SNAPSHOT_REQUIRED",
        "a future revision asks for a fresh snapshot"
    );
}

#[tokio::test]
async fn a_waiting_agent_wakes_when_a_machine_reports_a_change() {
    let daemon = daemon().await;
    let agent = daemon.create("claude", "OBSERVE").await;
    let token = agent.token.expose().to_string();
    let (_, body) = daemon
        .call(Method::GET, "/api/v1/agent/snapshot", Some(&token), None)
        .await;
    let revision = serde_json::from_slice::<ControlProjectionSnapshot>(&body)
        .unwrap()
        .view_revision;

    let app = daemon.app.clone();
    let path = format!("/api/v1/agent/updates?after={revision}&wait_ms=10000");
    let waiting = tokio::spawn(async move {
        let request = Request::get(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        (
            Instant::now(),
            to_bytes(response.into_body(), usize::MAX).await.unwrap(),
        )
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    let changed_at = Instant::now();
    worker_reports_a_session(&daemon.control);
    let (answered_at, body) = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap();
    assert!(
        answered_at.duration_since(changed_at) < Duration::from_secs(2),
        "woken by the change, not the timeout"
    );
    let updates: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        updates["messages"][0]["type"], "PROJECTION_DELTA",
        "{updates}"
    );
    assert!(updates.to_string().contains("Training"));
}

#[tokio::test]
async fn llms_txt_describes_the_agent_api_without_any_state() {
    let daemon = daemon().await;
    let (status, body) = daemon.call(Method::GET, "/llms.txt", None, None).await;
    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8(body).unwrap();
    assert!(text.starts_with("# Sentinal") && text.contains("sentinal mcp"));
    assert!(!text.contains(CONTROL_ID) && !text.contains("mac-mini"));
}
