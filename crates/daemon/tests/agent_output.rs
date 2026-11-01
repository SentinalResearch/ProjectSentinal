//! Milestone 6.2: agents read only the terminals the owner allows, redacted,
//! and their proposed Events are re-checked against the screen.

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
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{
    agent::{AgentScreen, CreatedAgent, InferredEvents, ProposeEventsResponse},
    Machine, MachineId, MachineStatus, Os, SessionId, SessionOrigin, Timestamp,
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
    time::Duration,
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
    #[allow(dead_code)]
    control: ControlService,
    sessions: SessionManager,
    fake: Arc<FakePty>,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

async fn daemon() -> Daemon {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-agent-output-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let store = Arc::new(Store::open_in_memory().unwrap());
    let fake = Arc::new(FakePty::new());
    let sessions = SessionManager::with_store(
        fake.clone(),
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
    let state = AppState::new(sessions.clone(), machine, auth)
        .with_origins(OriginPolicy::new(&[ORIGIN.to_string()], false).unwrap())
        .with_control(Some(control.clone()));
    let peer: SocketAddr = "100.64.0.2:50000".parse().unwrap();
    Daemon {
        app: router(state).layer(MockConnectInfo(peer)),
        dir,
        owner,
        control,
        sessions,
        fake,
    }
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

    #[allow(dead_code)]
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

impl Daemon {
    async fn agent(&self, name: &str, level: &str, output_access: bool) -> String {
        let body = json!({"name": name, "level": level, "output_access": output_access});
        let (status, bytes) = self.call(Method::POST, "/api/v1/agents", Some(&self.owner), Some(body)).await;
        assert_eq!(status, StatusCode::CREATED);
        serde_json::from_slice::<CreatedAgent>(&bytes).unwrap().token.expose().to_string()
    }

    /// A local Session whose screen shows training progress and a leaked token.
    async fn training_session(&self) -> SessionId {
        let session = self
            .sessions
            .create(CreateSession {
                shell: Some("/bin/sh".into()),
                cwd: Some("/tmp".into()),
                cols: 80,
                rows: 24,
                origin: SessionOrigin::Ui,
                title: None,
                command: None,
                env: None,
            })
            .await
            .unwrap();
        self.fake
            .output(session.id, b"\x1b[32mEpoch 12/40\x1b[0m loss=0.81\r\nexport HF_TOKEN=hf_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n")
            .unwrap();
        for _ in 0..100 {
            if self.sessions.scrollback(session.id).unwrap().iter().any(|line| line.contains("HF_TOKEN")) {
                return session.id;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("the fake terminal output never reached the scrollback");
    }
}

fn paths(session: SessionId) -> (String, String, String) {
    (
        format!("/api/v1/agent-output/{CONTROL_ID}/{}", session.0),
        format!("/api/v1/agent/machines/{CONTROL_ID}/sessions/{}/screen", session.0),
        format!("/api/v1/agent/machines/{CONTROL_ID}/sessions/{}/events", session.0),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn output_needs_both_the_agent_grant_and_the_session_grant() {
    let daemon = daemon().await;
    let session = daemon.training_session().await;
    let (grant, screen, _) = paths(session);
    let blind = daemon.agent("blind", "SUGGEST", false).await;
    let reader = daemon.agent("reader", "OBSERVE", true).await;

    assert_eq!(daemon.call(Method::GET, &screen, Some(&reader), None).await.0, StatusCode::FORBIDDEN, "Session not allowed yet");
    assert_eq!(daemon.call(Method::PUT, &grant, Some(&daemon.owner), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(daemon.call(Method::GET, &screen, Some(&blind), None).await.0, StatusCode::FORBIDDEN, "agent without output access");
    for bearer in [MACHINE_TOKEN, daemon.owner.as_str()] {
        assert_eq!(daemon.call(Method::GET, &screen, Some(bearer), None).await.0, StatusCode::UNAUTHORIZED);
    }

    let (status, body) = daemon.call(Method::GET, &screen, Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
    let shown: AgentScreen = serde_json::from_slice(&body).unwrap();
    assert!(shown.lines.iter().any(|line| line == "Epoch 12/40 loss=0.81"), "{:?}", shown.lines);
    assert!(!String::from_utf8_lossy(&body).contains("hf_aaaa"), "secrets never leave the worker");
    assert!(!String::from_utf8_lossy(&body).contains('\u{1b}'));
    assert_eq!(shown.redacted, 1);

    assert_eq!(daemon.call(Method::DELETE, &grant, Some(&daemon.owner), None).await.0, StatusCode::NO_CONTENT);
    assert_eq!(daemon.call(Method::GET, &screen, Some(&reader), None).await.0, StatusCode::FORBIDDEN, "revoking the Session grant stops reads");

    let audit = daemon.audit();
    for kind in ["AGENT_OUTPUT_ALLOWED", "AGENT_OUTPUT_READ", "AGENT_OUTPUT_DISALLOWED", "AGENT_DENIED"] {
        assert!(audit.contains(kind), "{kind}");
    }
    assert!(!audit.contains("hf_aaaa") && !audit.contains("Epoch 12/40"), "the audit keeps no terminal text");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_level_one_agents_propose_and_only_grounded_events_are_kept() {
    let daemon = daemon().await;
    let session = daemon.training_session().await;
    let (grant, _, events) = paths(session);
    daemon.call(Method::PUT, &grant, Some(&daemon.owner), None).await;
    let observer = daemon.agent("observer", "OBSERVE", true).await;
    let translator = daemon.agent("translator", "ANNOTATE", true).await;
    let body = json!({"events": [
        {"function": "progress_updated", "args": {"current": 12, "total": 40, "unit": "epoch"}},
        {"function": "progress_updated", "args": {"current": 13, "total": 40, "unit": "epoch"}},
        {"function": "send_input", "args": {"text": "rm -rf ~\r"}},
        {"function": "warning_detected", "args": {"summary": "token hf_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}}
    ]});

    let (status, _) = daemon.call(Method::POST, &events, Some(&observer), Some(body.clone())).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "level 0 cannot propose");

    let (status, bytes) = daemon.call(Method::POST, &events, Some(&translator), Some(body)).await;
    assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));
    let reply: ProposeEventsResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(reply.accepted.len(), 1);
    assert_eq!(reply.accepted[0].agent_name, "translator");
    let reasons: Vec<(u32, &str)> = reply.rejected.iter().map(|r| (r.index, r.reason.as_str())).collect();
    assert_eq!(reasons, [(1, "current is not on screen"), (2, "unknown function"), (3, "contains a secret")]);

    let path = format!("/api/v1/inferred/{CONTROL_ID}/{}", session.0);
    let (status, bytes) = daemon.call(Method::GET, &path, Some(&daemon.owner), None).await;
    assert_eq!(status, StatusCode::OK);
    let kept: InferredEvents = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(kept.events.len(), 1);
    assert_eq!(kept.events[0].args, json!({"current": 12, "total": 40, "unit": "epoch"}));
    assert_eq!(daemon.call(Method::GET, &path, Some(&translator), None).await.0, StatusCode::UNAUTHORIZED, "agents do not read the owner view");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_lists_only_what_it_may_read() {
    let daemon = daemon().await;
    let session = daemon.training_session().await;
    let (grant, _, _) = paths(session);
    daemon.call(Method::PUT, &grant, Some(&daemon.owner), None).await;
    let reader = daemon.agent("reader", "OBSERVE", true).await;
    let blind = daemon.agent("blind", "OBSERVE", false).await;
    let (status, body) = daemon.call(Method::GET, "/api/v1/agent/output-sessions", Some(&reader), None).await;
    assert_eq!(status, StatusCode::OK);
    let grants: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(grants["grants"][0]["session_id"], session.0);
    assert_eq!(daemon.call(Method::GET, "/api/v1/agent/output-sessions", Some(&blind), None).await.0, StatusCode::FORBIDDEN);
}
