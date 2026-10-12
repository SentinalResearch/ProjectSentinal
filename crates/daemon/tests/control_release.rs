//! `/api/v1/control/release` on the full daemon router: owner-only release
//! state, check-now, policy, and the detached control update. The `sentinal`
//! CLI is replaced by a fake script that writes the same state files.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]

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
    control::{ControlRelease, ControlReleaseSettings, ControlService, ControlSettings, Detach},
    origin::OriginPolicy,
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{Machine, MachineId, MachineStatus, Os, Timestamp};
use serde_json::{json, Value};
use std::{
    fs,
    net::SocketAddr,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use store::Store;
use tower::ServiceExt;

const TOKEN: &str = "machine-token-for-tests";
const ORIGIN: &str = "https://sentinal-control.example.ts.net";
static NEXT: AtomicU64 = AtomicU64::new(1);

struct Daemon {
    app: Router,
    dir: PathBuf,
    live: Arc<AtomicU32>,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A fake `sentinal` that logs its arguments and writes the CLI's state files.
const FAKE_CLI: &str = r#"#!/bin/sh
# This script lives at <test dir>/prefix/bin/sentinal.
root="$(cd "$(dirname "$0")/../.." && pwd)"
printf '%s\n' "$*" >>"$root/cli.log"
data="$root/data"
case "$1 $2" in
  'release check')
    printf '%s' '{"checked_at":1792000000000,"current":"0.1.17","channel":"stable","available":{"version":"0.1.18","signed":true,"published_at":"2026-10-12T10:00:00Z","notes_url":"https://github.com/o/r/releases/tag/v0.1.18"},"unsigned_found":null,"last_result":null}' >"$data/release-watch.json"
    echo '{}' ;;
  'release policy')
    policy=NOTIFY; channel=stable
    case "$3" in auto) policy=AUTOMATIC_WHEN_IDLE ;; off) policy=OFF ;; esac
    case "$*" in *'--channel beta'*) channel=beta ;; esac
    printf '{"policy":"%s","channel":"%s","window":"03:00-05:00"}' "$policy" "$channel" >"$data/release-policy.json" ;;
  'release update')
    [ "${SENTINAL_TEST_UPDATE:-ok}" = ok ] || exit 1
    printf '%s' '{"checked_at":1792000000000,"current":"0.1.18","channel":"stable","available":null,"unsigned_found":null,"last_result":{"state":"UPDATED","version":"0.1.18","previous":"0.1.17","reason":null,"at":99999999999999}}' >"$data/release-watch.json" ;;
  *) exit 2 ;;
esac
"#;

fn daemon(control: bool) -> Daemon {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-control-release-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let data = dir.join("data");
    let prefix = dir.join("prefix");
    fs::create_dir_all(&data).unwrap();
    fs::create_dir_all(prefix.join("share/sentinal")).unwrap();
    fs::create_dir_all(prefix.join("bin")).unwrap();
    fs::write(
        prefix.join("share/sentinal/release.json"),
        r#"{"schema_version":1,"version":"0.1.17","target":"aarch64-apple-darwin"}"#,
    )
    .unwrap();
    let cli = prefix.join("bin/sentinal");
    fs::write(&cli, FAKE_CLI).unwrap();
    fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();

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
    let live = Arc::new(AtomicU32::new(0));
    let (control_service, release) = if control {
        let service = ControlService::open(
            store,
            ControlSettings {
                public_url: ORIGIN.into(),
                heartbeat_interval_ms: 30_000,
                development: false,
                machine_id: MachineId("01J9Y0CTRL0000000000000000".into()),
                display_name: "Control".into(),
                os: Os::Macos,
                daemon_version: "0.1.17".into(),
                secrets_file: dir.join("machine-credentials.json"),
                audit_dir: dir.clone(),
                read_model_enabled: true,
            },
            Arc::new(|| Timestamp(1_790_900_000_000)),
        )
        .unwrap();
        let counter = live.clone();
        let release = ControlRelease::new(ControlReleaseSettings {
            data_dir: data.clone(),
            config_dir: dir.join("config"),
            prefix: prefix.clone(),
            cli,
            detach: Detach::ProcessGroup,
            live_full_sessions: Arc::new(move || counter.load(Ordering::Relaxed)),
        });
        (Some(service), Some(release))
    } else {
        (None, None)
    };
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
        .with_control(control_service)
        .with_release(release);
    let peer: SocketAddr = "100.64.0.2:5000".parse().unwrap();
    Daemon {
        app: router(state).layer(MockConnectInfo(peer)),
        dir,
        live,
    }
}

impl Daemon {
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
                Some(json!({"password": "a-long-owner-password"})),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        body["token"].as_str().unwrap().into()
    }

    fn cli_log(&self) -> String {
        fs::read_to_string(self.dir.join("cli.log")).unwrap_or_default()
    }
}

#[tokio::test]
async fn release_state_is_owner_only_and_reflects_the_installed_release() {
    let d = daemon(true);
    assert_eq!(
        d.send("GET", "/api/v1/control/release", None, None).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        d.send("GET", "/api/v1/control/release", Some(TOKEN), None)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let owner = d.owner().await;
    d.live.store(2, Ordering::Relaxed);
    let (status, view) = d
        .send("GET", "/api/v1/control/release", Some(&owner), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["current"], "0.1.17");
    assert_eq!(view["policy"], "NOTIFY");
    assert_eq!(view["channel"], "STABLE");
    assert!(view["available"].is_null());
    assert_eq!(view["live_full_sessions"], 2);
    assert!(d.cli_log().is_empty(), "reading state must not run the CLI");
}

#[tokio::test]
async fn check_now_and_policy_go_through_the_cli() {
    let d = daemon(true);
    let owner = d.owner().await;
    let (status, view) = d
        .send("POST", "/api/v1/control/release/check", Some(&owner), None)
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["available"]["version"], "0.1.18");
    assert!(
        d.cli_log().contains("release check --json --config-dir"),
        "{}",
        d.cli_log()
    );

    let (status, view) = d
        .send(
            "PUT",
            "/api/v1/control/release/policy",
            Some(&owner),
            Some(json!({"policy":"AUTOMATIC_WHEN_IDLE","channel":"BETA"})),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{view}");
    assert_eq!(view["policy"], "AUTOMATIC_WHEN_IDLE");
    assert_eq!(view["channel"], "BETA");
    assert!(
        d.cli_log().contains("release policy auto --channel beta"),
        "{}",
        d.cli_log()
    );
}

#[tokio::test]
async fn update_starts_only_for_the_offered_signed_release_with_consent() {
    let d = daemon(true);
    let owner = d.owner().await;
    let update = |version: &str, allow: bool| json!({"version": version, "allow_interrupt": allow});

    // Nothing offered yet.
    let (status, _) = d
        .send(
            "POST",
            "/api/v1/control/release/update",
            Some(&owner),
            Some(update("0.1.18", true)),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    d.send("POST", "/api/v1/control/release/check", Some(&owner), None)
        .await;

    // A malformed version, or one that is not the offered release.
    for bad in ["0.1.18; rm", "0.1.19"] {
        let (status, _) = d
            .send(
                "POST",
                "/api/v1/control/release/update",
                Some(&owner),
                Some(update(bad, true)),
            )
            .await;
        assert!(
            status == StatusCode::BAD_REQUEST || status == StatusCode::CONFLICT,
            "{bad}: {status}"
        );
    }

    // Live FULL Sessions need explicit consent.
    d.live.store(1, Ordering::Relaxed);
    let (status, body) = d
        .send(
            "POST",
            "/api/v1/control/release/update",
            Some(&owner),
            Some(update("0.1.18", false)),
        )
        .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert!(
        body["message"]
            .as_str()
            .unwrap()
            .contains("1 live FULL Session"),
        "{body}"
    );
    assert!(!d.cli_log().contains("release update"));

    let (status, started) = d
        .send(
            "POST",
            "/api/v1/control/release/update",
            Some(&owner),
            Some(update("0.1.18", true)),
        )
        .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{started}");
    let update_id = started["update_id"].as_str().unwrap().to_string();
    assert!(update_id.starts_with("cu_"));

    // A second update while one runs is refused.
    let (status, _) = d
        .send(
            "POST",
            "/api/v1/control/release/update",
            Some(&owner),
            Some(update("0.1.18", true)),
        )
        .await;
    assert!(status == StatusCode::CONFLICT || status == StatusCode::ACCEPTED);

    let mut state = Value::Null;
    for _ in 0..60 {
        let (_, view) = d
            .send("GET", "/api/v1/control/release", Some(&owner), None)
            .await;
        state = view["update"].clone();
        if state["state"] == "UPDATED" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(state["state"], "UPDATED", "{state}");
    assert_eq!(state["update_id"], update_id.as_str());
    assert_eq!(state["previous"], "0.1.17");
    assert!(
        d.cli_log()
            .contains("release update --version 0.1.18 --yes --config-dir"),
        "{}",
        d.cli_log()
    );
}

#[tokio::test]
async fn a_worker_has_no_release_routes() {
    let d = daemon(false);
    let owner = d.owner().await;
    let (status, body) = d
        .send("GET", "/api/v1/control/release", Some(&owner), None)
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(body["code"], "NOT_CONTROL");
}
