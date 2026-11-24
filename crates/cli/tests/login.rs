//! `sentinal login` / `logout` and the remote list against a stand-in control
//! that speaks the real control's contract: `POST /api/v1/auth/login`
//! returns an expiring owner token, `GET /api/v1/control/snapshot` needs it,
//! `POST /api/v1/auth/logout` ends it (T4.2). The real control's side is in
//! `crates/daemon/tests/enrollment_api.rs`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use cli::login::{self, exit, OwnerSession};
use protocol::Timestamp;
use serde_json::{json, Value};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

const PASSWORD: &str = "correct horse battery staple";
const TOKEN: &str = "owner-token-from-the-control";

fn authorized(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        == Some(&format!("Bearer {TOKEN}"))
}

async fn control(logouts: Arc<AtomicUsize>) -> String {
    let app = Router::new()
        .route(
            "/api/v1/auth/login",
            post(|Json(body): Json<Value>| async move {
                if body["password"] == PASSWORD {
                    (StatusCode::OK, Json(json!({ "token": TOKEN, "expires_at": 4_102_444_800_000_i64 })))
                } else {
                    (StatusCode::UNAUTHORIZED, Json(json!({ "code": "INVALID_PASSWORD" })))
                }
            }),
        )
        .route(
            "/api/v1/control/snapshot",
            get(|headers: HeaderMap| async move {
                if !authorized(&headers) {
                    return (StatusCode::UNAUTHORIZED, Json(Value::Null));
                }
                (
                    StatusCode::OK,
                    Json(json!({
                        "schema_version": 1, "view_revision": 3, "generated_at": 1,
                        "machines": [], "omitted_machines": 0, "omitted_sessions": 0, "truncated": false
                    })),
                )
            }),
        )
        .route(
            "/api/v1/auth/logout",
            post(move |headers: HeaderMap| {
                let logouts = logouts.clone();
                async move {
                    if authorized(&headers) {
                        logouts.fetch_add(1, Ordering::SeqCst);
                        StatusCode::NO_CONTENT
                    } else {
                        StatusCode::UNAUTHORIZED
                    }
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sentinal-login-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn login_keeps_the_owner_token_privately_and_logout_ends_it_at_the_control() {
    let logouts = Arc::new(AtomicUsize::new(0));
    let url = control(logouts.clone()).await;
    let dir = temp_dir("flow");
    let token_file = dir.join("token");
    let session_file = login::session_path(&token_file);

    // A wrong password keeps nothing.
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = login::login(
        Some(url.clone()),
        &token_file,
        || Ok(Some("nope".into())),
        &mut out,
        &mut err,
    )
    .await;
    assert_eq!(code, exit::REFUSED);
    assert!(String::from_utf8_lossy(&err).contains("wrong password"));
    assert!(!session_file.exists());

    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = login::login(
        Some(format!("{url}/")),
        &token_file,
        || Ok(Some(PASSWORD.into())),
        &mut out,
        &mut err,
    )
    .await;
    assert_eq!(code, exit::OK, "{}", String::from_utf8_lossy(&err));
    assert!(String::from_utf8_lossy(&out).contains("Signed in"));
    let mode = std::fs::metadata(&session_file)
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the owner token is readable only by its owner");
    let kept = std::fs::read_to_string(&session_file).unwrap();
    assert!(!kept.contains(PASSWORD), "the password is never kept");

    // The kept sign-in reads the control's list.
    let session: OwnerSession = login::load(&session_file, Timestamp(1)).unwrap();
    assert_eq!(session.control_url, url);
    let snapshot = login::snapshot(&session).await.unwrap();
    assert_eq!(snapshot.view_revision, 3);

    // A token the control no longer accepts reads as signed out.
    let stale = OwnerSession {
        token: "ended".into(),
        ..session.clone()
    };
    assert!(matches!(
        login::snapshot(&stale).await,
        Err(login::RemoteError::SignedOut)
    ));

    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        login::logout(&token_file, &mut out, &mut err).await,
        exit::OK
    );
    assert_eq!(logouts.load(Ordering::SeqCst), 1, "the control was told");
    assert!(!session_file.exists());
    let (mut out, mut err) = (Vec::new(), Vec::new());
    assert_eq!(
        login::logout(&token_file, &mut out, &mut err).await,
        exit::OK
    );
    assert!(String::from_utf8_lossy(&out).contains("Not signed in"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn an_unreachable_control_says_so() {
    let dir = temp_dir("unreachable");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    // Nothing listens on port 9 of loopback.
    let code = login::login(
        Some("http://127.0.0.1:9".into()),
        &dir.join("token"),
        || Ok(Some(PASSWORD.into())),
        &mut out,
        &mut err,
    )
    .await;
    assert_eq!(code, exit::UNREACHABLE);
    std::fs::remove_dir_all(dir).unwrap();
}
