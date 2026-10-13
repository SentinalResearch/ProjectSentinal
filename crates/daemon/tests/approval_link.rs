//! Worker ↔ control approval pairing over real loopback HTTP (development
//! mode). The control's proof callback reaches the worker's real
//! `pairing_proof`; only `tailscale whois` is faked.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    auth::AuthManager,
    control::{
        routes::ControlRoutes, ControlService, ControlSettings, PairingVerifier, VerifyFuture,
    },
    identity,
    link::{LinkManager, LinkSettings},
};
use protocol::{
    link::{LinkEndpoint, LocalLinkState},
    pairing::{PairingApproval, PairingProof, PairingRequestState, TailscaleIdentity},
    LocalErrorCode, LocalRequest, LocalResponse, MachineId, Os, Timestamp,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
};
use store::Store;

const CONTROL: &str = "01J9Y0C7RQ9M3X8V6T4N2P0SDB";
const OTHER_CONTROL: &str = "01K2C7RQ9M3X8V6T4N2P0SDB5A";
static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-approval-link-{name}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

/// Calls the worker in-process, as the Tailnet callback would.
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
                node: "pop-os".into(),
                login: "owner@example.com".into(),
                os: "linux".into(),
                address: address.into(),
            })
        })
    }
}

struct Pair {
    control: ControlService,
    worker: LinkManager,
    url: String,
    dirs: Vec<PathBuf>,
    server: tokio::task::JoinHandle<()>,
}

impl Drop for Pair {
    fn drop(&mut self) {
        self.server.abort();
        for dir in &self.dirs {
            let _ = fs::remove_dir_all(dir);
        }
    }
}

async fn pair() -> Pair {
    let control_dir = temp_dir("control");
    let worker_dir = temp_dir("worker");
    let slot = Arc::new(OnceLock::new());
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
            package_dir: None,
            os: Os::Macos,
            daemon_version: "0.1.18".into(),
            secrets_file: control_dir.join("machine-credentials.json"),
            audit_dir: control_dir.clone(),
            read_model_enabled: true,
        },
        clock.clone(),
    )
    .unwrap()
    .with_pairing_verifier(Arc::new(Callback(slot.clone())));
    let auth = AuthManager::load(&control_dir.join("auth.json"), "machine-token".into()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = ControlRoutes::new(Some(control.clone()), auth, false, false).router();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
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
                port: 7431,
                dns_name: None,
            },
            os: Os::Linux,
            daemon_version: "0.1.18".into(),
            development: true,
            audit_dir: worker_dir.clone(),
            background: false,
        },
        clock,
    );
    slot.set(worker.clone()).ok();
    Pair {
        control,
        worker,
        url,
        dirs: vec![control_dir, worker_dir],
        server,
    }
}

impl Pair {
    async fn request(&self, control_id: &str) -> LocalResponse {
        self.worker
            .handle_local(LocalRequest::RequestApproval {
                control_url: self.url.clone(),
                control_machine_id: MachineId(control_id.into()),
                display_name: Some("RTX PC".into()),
            })
            .await
    }

    async fn claim(&self) -> LocalResponse {
        self.worker.handle_local(LocalRequest::ClaimApproval).await
    }
}

#[tokio::test]
async fn both_sides_show_the_same_words_and_approval_connects_the_worker() {
    let pair = pair().await;
    let LocalResponse::ApprovalRequested {
        request_id,
        verification_words,
        ..
    } = pair.request(CONTROL).await
    else {
        panic!("request was not created");
    };
    let owner_view = pair.control.pairing_requests().unwrap();
    assert_eq!(owner_view.requests[0].request_id, request_id);
    assert_eq!(
        owner_view.requests[0].verification_words,
        verification_words
    );
    assert!(owner_view.requests[0].verified);

    assert_eq!(
        pair.claim().await,
        LocalResponse::ApprovalState {
            state: PairingRequestState::Pending
        }
    );
    pair.control
        .approve_pairing_request(
            &request_id,
            &PairingApproval {
                display_name: Some("Lab RTX".into()),
            },
        )
        .unwrap();
    let LocalResponse::Connection { connection } = pair.claim().await else {
        panic!("claim did not connect");
    };
    assert_eq!(connection.state, LocalLinkState::Active);
    assert_eq!(connection.display_name.as_deref(), Some("Lab RTX"));
    assert_eq!(
        connection.control_machine_id,
        Some(MachineId(CONTROL.into()))
    );
    assert!(
        pair.worker.pairing_proof(&request_id).is_none(),
        "nothing stays open after the claim"
    );

    let audit = fs::read_to_string(pair.dirs[1].join("link-audit.jsonl")).unwrap();
    let longest_hex_run = audit
        .split(|c: char| !c.is_ascii_hexdigit())
        .map(str::len)
        .max()
        .unwrap_or(0);
    assert!(
        longest_hex_run < 64,
        "no secret or hash in the audit: {audit}"
    );
}

#[tokio::test]
async fn a_different_control_is_refused_and_a_rejection_ends_the_request() {
    let pair = pair().await;
    let refused = pair.request(OTHER_CONTROL).await;
    // The worker's proof names the control it discovered, so the control refuses.
    assert!(
        matches!(
            refused,
            LocalResponse::Error {
                code: LocalErrorCode::CodeRejected,
                ..
            }
        ),
        "{refused:?}"
    );
    assert!(matches!(
        pair.claim().await,
        LocalResponse::Error {
            code: LocalErrorCode::NotFound,
            ..
        }
    ));

    let LocalResponse::ApprovalRequested { request_id, .. } = pair.request(CONTROL).await else {
        panic!("request was not created");
    };
    pair.control.reject_pairing_request(&request_id).unwrap();
    assert_eq!(
        pair.claim().await,
        LocalResponse::ApprovalState {
            state: PairingRequestState::Rejected
        }
    );
    assert!(!pair.dirs[1].join("link.json").exists());
}

#[tokio::test]
async fn cancelling_withdraws_the_request_on_the_control() {
    let pair = pair().await;
    let LocalResponse::ApprovalRequested { request_id, .. } = pair.request(CONTROL).await else {
        panic!("request was not created");
    };
    assert_eq!(
        pair.worker.handle_local(LocalRequest::CancelApproval).await,
        LocalResponse::ApprovalState {
            state: PairingRequestState::Cancelled
        }
    );
    let view = pair.control.pairing_requests().unwrap();
    assert_eq!(view.requests[0].request_id, request_id);
    assert_eq!(view.requests[0].state, PairingRequestState::Cancelled);
    assert!(pair.worker.pairing_proof(&request_id).is_none());
}

#[test]
fn proof_answers_only_well_formed_ids() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let pair = pair().await;
        assert!(
            pair.worker.pairing_proof("pr_0000000000000001").is_none(),
            "nothing open"
        );
        assert!(pair.worker.pairing_proof("../etc").is_none());
    });
}
