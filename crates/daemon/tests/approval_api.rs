//! Click-to-approve pairing on the control (4.E §6): verified requests only,
//! owner-only decisions, one claim with the secret, rate limits, and an audit
//! with no secrets. The callback proof and `tailscale whois` are faked.

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
    control::{ControlService, ControlSettings, PairingVerifier, VerifyFuture},
    origin::OriginPolicy,
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{
    link::{LinkEndpoint, LinkMachineInfo, MachineCapability},
    pairing::{
        PairingProof, PairingRequestCreate, PairingRequestCreated, PairingRequestList,
        PairingRequestState, TailscaleIdentity,
    },
    Machine, MachineId, MachineStatus, Os, Timestamp,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
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
const ORIGIN: &str = "https://sentinal-control.example.ts.net";
const CONTROL_ID: &str = "01J9Y0C7RQ9M3X8V6T4N2P0SDB";
const WORKER_ID: &str = "01M40Q6XMY22VM973434SCPS90";
static NEXT: AtomicU64 = AtomicU64::new(1);

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha(text: &str) -> String {
    hex(&Sha256::digest(text.as_bytes()))
}

/// Answers the callback as the requesting worker would (or wouldn't).
struct Verifier {
    secret_sha256: String,
    proof_ok: bool,
    whois_ok: bool,
}

impl PairingVerifier for Verifier {
    fn proof<'a>(
        &'a self,
        _endpoint: &'a LinkEndpoint,
        request_id: &'a str,
    ) -> VerifyFuture<'a, PairingProof> {
        Box::pin(async move {
            if !self.proof_ok {
                return Err("connection refused".into());
            }
            Ok(PairingProof {
                request_id: request_id.into(),
                request_secret_sha256: self.secret_sha256.clone(),
                control_machine_id: MachineId(CONTROL_ID.into()),
            })
        })
    }

    fn whois<'a>(&'a self, address: &'a str) -> VerifyFuture<'a, TailscaleIdentity> {
        Box::pin(async move {
            if !self.whois_ok {
                return Err("whois failed".into());
            }
            Ok(TailscaleIdentity {
                node: "macbook".into(),
                login: "owner@example.com".into(),
                os: "macOS".into(),
                address: address.into(),
            })
        })
    }
}

struct Control {
    app: Router,
    dir: PathBuf,
    secret: String,
}

impl Drop for Control {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn control(proof_ok: bool, whois_ok: bool) -> Control {
    let secret = "5e".repeat(32);
    let dir = std::env::temp_dir().join(format!(
        "sentinal-approval-{}-{}",
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
    let service = ControlService::open(
        store,
        ControlSettings {
            public_url: ORIGIN.into(),
            heartbeat_interval_ms: 30_000,
            development: false,
            machine_id: MachineId(CONTROL_ID.into()),
            display_name: "control".into(),
            discovery_name: None,
            os: Os::Macos,
            daemon_version: "0.1.18".into(),
            secrets_file: dir.join("machine-credentials.json"),
            audit_dir: dir.clone(),
            read_model_enabled: true,
        },
        Arc::new(|| Timestamp(1_790_900_000_000)),
    )
    .unwrap()
    .with_pairing_verifier(Arc::new(Verifier {
        secret_sha256: sha(&secret),
        proof_ok,
        whois_ok,
    }));
    let machine = Machine {
        id: MachineId(CONTROL_ID.into()),
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
        .with_control(Some(service))
        .with_tailscale_serve(true);
    let peer: SocketAddr = "127.0.0.1:50000".parse().unwrap();
    Control {
        app: router(state).layer(MockConnectInfo(peer)),
        dir,
        secret,
    }
}

impl Control {
    async fn send(
        &self,
        method: &str,
        path: &str,
        bearer: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
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
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn owner(&self) -> String {
        let (status, body) = self
            .send(
                "POST",
                "/api/v1/auth/setup",
                Some(TOKEN),
                Some(json!({ "password": "a-long-owner-password" })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        body["token"].as_str().unwrap().to_string()
    }

    fn create_body(&self, machine_id: &str) -> Value {
        serde_json::to_value(PairingRequestCreate {
            machine: LinkMachineInfo {
                machine_id: MachineId(machine_id.into()),
                display_name: "Owner's MacBook".into(),
                os: Os::Macos,
                daemon_version: "0.1.18".into(),
                capabilities: vec![MachineCapability::Sessions],
            },
            endpoint: LinkEndpoint {
                address: "100.101.29.67".into(),
                port: 7420,
                dns_name: None,
            },
            request_secret_sha256: sha(&self.secret),
            worker_nonce: "ab".repeat(16),
        })
        .unwrap()
    }

    async fn create(&self) -> (StatusCode, Value) {
        self.send(
            "POST",
            "/api/v1/pairing/requests",
            None,
            Some(self.create_body(WORKER_ID)),
        )
        .await
    }

    async fn claim(&self, id: &str, secret: &str) -> (StatusCode, Value) {
        self.send(
            "POST",
            &format!("/api/v1/pairing/requests/{id}/claim"),
            None,
            Some(json!({ "request_secret": secret, "control_credential": "c".repeat(64) })),
        )
        .await
    }
}

#[tokio::test]
async fn a_verified_request_is_approved_by_the_owner_and_claimed_once() {
    let control = control(true, true);
    let (status, body) = control.create().await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let created: PairingRequestCreated = serde_json::from_value(body).unwrap();
    let id = created.request_id.clone();
    assert!(protocol::pairing::is_request_id(&id));

    let owner = control.owner().await;
    let (status, list) = control
        .send("GET", "/api/v1/pairing/requests", Some(&owner), None)
        .await;
    assert_eq!(status, StatusCode::OK);
    let list: PairingRequestList = serde_json::from_value(list).unwrap();
    let view = &list.requests[0];
    assert!(view.verified);
    assert_eq!(view.state, PairingRequestState::Pending);
    assert_eq!(view.tailscale.as_ref().unwrap().node, "macbook");
    let transcript = protocol::pairing::verification_transcript(
        &MachineId(CONTROL_ID.into()),
        &id,
        &created.control_nonce,
        &sha(&control.secret),
        &"ab".repeat(16),
    );
    assert_eq!(
        view.verification_words,
        protocol::pairing::verification_words(&Sha256::digest(&transcript))
    );

    let (status, body) = control.claim(&id, &control.secret).await;
    assert_eq!(status, StatusCode::ACCEPTED, "pending: {body}");
    assert_eq!(body["state"], "PENDING");

    let (status, _) = control
        .send(
            "POST",
            &format!("/api/v1/pairing/requests/{id}/approve"),
            Some(&owner),
            Some(json!({"display_name": "Lab MacBook"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK);

    let (status, body) = control.claim(&id, &"0f".repeat(32)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
    assert_eq!(body["code"], "INVALID_CLAIM");

    let (status, body) = control.claim(&id, &control.secret).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["state"], "CLAIMED");
    assert_eq!(body["link"]["display_name"], "Lab MacBook");
    assert_eq!(body["link"]["control_machine_id"], CONTROL_ID);

    let (status, _) = control.claim(&id, &control.secret).await;
    assert_ne!(status, StatusCode::OK, "one success only");

    let (_, registry) = control
        .send("GET", "/api/v1/machines", Some(&owner), None)
        .await;
    assert!(registry.to_string().contains("Lab MacBook"), "{registry}");

    let audit = fs::read_to_string(control.dir.join("pairing-audit.jsonl")).unwrap();
    for kind in [
        "REQUESTED",
        "PROOF_OK",
        "APPROVED",
        "CLAIMED",
        "CLAIM_REFUSED",
    ] {
        assert!(audit.contains(kind), "{kind}: {audit}");
    }
    for secret in [
        control.secret.clone(),
        sha(&control.secret),
        "ab".repeat(16),
        created.control_nonce.clone(),
    ] {
        assert!(!audit.contains(&secret), "audit leaks a secret or nonce");
    }
}

#[tokio::test]
async fn unverifiable_requests_are_refused_and_never_approvable() {
    for (proof_ok, whois_ok) in [(false, true), (true, false)] {
        let control = control(proof_ok, whois_ok);
        let (status, body) = control.create().await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        let owner = control.owner().await;
        let (_, list) = control
            .send("GET", "/api/v1/pairing/requests", Some(&owner), None)
            .await;
        let list: PairingRequestList = serde_json::from_value(list).unwrap();
        assert!(list
            .requests
            .iter()
            .all(|request| !request.verified && request.state != PairingRequestState::Pending));
        if let Some(request) = list.requests.first() {
            let (status, _) = control
                .send(
                    "POST",
                    &format!("/api/v1/pairing/requests/{}/approve", request.request_id),
                    Some(&owner),
                    Some(json!({})),
                )
                .await;
            assert_ne!(status, StatusCode::OK);
        }
    }

    // A proof for a different secret (another node answering) is not a proof.
    let mut control = control(true, true);
    control.secret = "77".repeat(32);
    let (status, _) = control.create().await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn rejected_and_cancelled_requests_end_their_claims() {
    let control = control(true, true);
    let owner = control.owner().await;
    let (_, body) = control.create().await;
    let id = body["request_id"].as_str().unwrap().to_string();
    let (status, _) = control
        .send(
            "POST",
            &format!("/api/v1/pairing/requests/{id}/reject"),
            Some(&owner),
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = control.claim(&id, &control.secret).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["state"], "REJECTED");

    let (_, body) = control.create().await;
    let id = body["request_id"].as_str().unwrap().to_string();
    let (status, _) = control
        .send(
            "DELETE",
            &format!("/api/v1/pairing/requests/{id}"),
            None,
            Some(json!({"request_secret": "0f".repeat(32)})),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = control
        .send(
            "DELETE",
            &format!("/api/v1/pairing/requests/{id}"),
            None,
            Some(json!({"request_secret": control.secret})),
        )
        .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = control.claim(&id, &control.secret).await;
    assert_eq!(status, StatusCode::GONE);
    assert_eq!(body["state"], "CANCELLED");
}

#[tokio::test]
async fn decisions_need_the_owner_and_requests_are_rate_limited() {
    let control = control(true, true);
    let (_, body) = control.create().await;
    let id = body["request_id"].as_str().unwrap().to_string();
    let approve = format!("/api/v1/pairing/requests/{id}/approve");
    assert_eq!(
        control
            .send("GET", "/api/v1/pairing/requests", None, None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        control
            .send("POST", &approve, None, Some(json!({})))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    control.owner().await;
    assert_eq!(
        control
            .send("POST", &approve, Some(TOKEN), Some(json!({})))
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    for _ in 0..4 {
        assert_eq!(control.create().await.0, StatusCode::CREATED);
    }
    let (status, body) = control.create().await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "PAIRING_BUSY");
}

#[tokio::test]
async fn an_actively_linked_machine_or_the_control_itself_cannot_request() {
    let control = control(true, true);
    let owner = control.owner().await;
    let (_, body) = control.create().await;
    let id = body["request_id"].as_str().unwrap().to_string();
    control
        .send(
            "POST",
            &format!("/api/v1/pairing/requests/{id}/approve"),
            Some(&owner),
            Some(json!({})),
        )
        .await;
    assert_eq!(control.claim(&id, &control.secret).await.0, StatusCode::OK);
    let (status, body) = control.create().await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "MACHINE_ALREADY_LINKED");

    let (status, _) = control
        .send(
            "POST",
            "/api/v1/pairing/requests",
            None,
            Some(control.create_body(CONTROL_ID)),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[test]
fn the_daemon_derives_the_shared_vector_words() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../protocol/tests/fixtures/pairing.json");
    let fixture: Value = serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    let vector = &fixture["verification_vector"];
    let text = |key: &str| vector[key].as_str().unwrap().to_string();
    let words = daemon::control::verification_words(
        &MachineId(text("control_machine_id")),
        &text("request_id"),
        &text("control_nonce"),
        &text("request_secret_sha256"),
        &text("worker_nonce"),
    );
    let expected: Vec<String> = serde_json::from_value(vector["words"].clone()).unwrap();
    assert_eq!(words, expected);
}
