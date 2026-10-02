//! Exact Origin allow-list for HTTP routes and the WebSocket upgrade.

#![allow(clippy::unwrap_used)]

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use daemon::origin::{OriginPolicy, OriginRejection};
use tower::ServiceExt;

fn policy() -> OriginPolicy {
    OriginPolicy::new(
        &["https://sentinal-control.example.ts.net".to_string()],
        false,
    )
    .unwrap()
}

#[test]
fn exact_origin_matches_case_insensitively_with_default_port_normalized() {
    let policy = policy();
    for allowed in [
        "https://sentinal-control.example.ts.net",
        "HTTPS://Sentinal-Control.Example.TS.NET",
        "https://sentinal-control.example.ts.net:443",
    ] {
        assert_eq!(policy.check(Some(allowed)), Ok(()), "{allowed}");
    }
}

#[test]
fn deceptive_and_mismatched_origins_are_rejected() {
    let policy = policy();
    for rejected in [
        "https://sentinal-control.example.ts.net.evil.com",
        "https://evil-sentinal-control.example.ts.net",
        "https://xsentinal-control.example.ts.net",
        "https://control.example.ts.net",
        "http://sentinal-control.example.ts.net",
        "https://sentinal-control.example.ts.net:8443",
        "https://sentinal-control.example.ts.net/",
        "https://user@sentinal-control.example.ts.net",
        "null",
        "",
        "*",
    ] {
        assert!(policy.check(Some(rejected)).is_err(), "{rejected}");
    }
}

#[test]
fn null_origin_is_always_rejected() {
    assert_eq!(policy().check(Some("null")), Err(OriginRejection::Null));
}

#[test]
fn requests_without_an_origin_are_allowed() {
    assert_eq!(policy().check(None), Ok(()));
}

#[test]
fn empty_list_rejects_every_browser_origin() {
    let policy = OriginPolicy::new(&[], false).unwrap();
    assert!(policy
        .check(Some("https://sentinal-control.example.ts.net"))
        .is_err());
    assert_eq!(policy.check(None), Ok(()));
}

#[test]
fn wildcards_and_http_outside_development_are_refused_at_load() {
    for bad in [
        "*",
        "https://*.example.ts.net",
        "null",
        "https://a.example.ts.net/x",
    ] {
        assert!(
            OriginPolicy::new(&[bad.to_string()], true).is_err(),
            "{bad}"
        );
    }
    assert!(OriginPolicy::new(&["http://127.0.0.1:4173".to_string()], false).is_err());
    let development = OriginPolicy::new(&["http://127.0.0.1:4173".to_string()], true).unwrap();
    assert_eq!(development.check(Some("http://127.0.0.1:4173")), Ok(()));
    assert!(development.check(Some("http://127.0.0.1:4174")).is_err());
    assert!(development.check(Some("http://localhost:4173")).is_err());
}

fn app(policy: OriginPolicy) -> Router {
    Router::new()
        .route("/api/v1/ws", get(|| async { "upgrade" }))
        .route("/api/v1/health", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(
            policy,
            daemon::origin::enforce,
        ))
}

async fn status(origin: Option<&str>, path: &str, websocket: bool) -> StatusCode {
    let mut request = Request::builder().uri(path);
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    if websocket {
        request = request
            .header("connection", "upgrade")
            .header("upgrade", "websocket");
    }
    app(policy())
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn middleware_applies_to_http_and_websocket_upgrades() {
    let good = "https://sentinal-control.example.ts.net";
    let evil = "https://sentinal-control.example.ts.net.evil.com";
    assert_eq!(
        status(Some(good), "/api/v1/health", false).await,
        StatusCode::OK
    );
    assert_eq!(status(Some(good), "/api/v1/ws", true).await, StatusCode::OK);
    assert_eq!(
        status(Some(evil), "/api/v1/health", false).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(Some(evil), "/api/v1/ws", true).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(Some("null"), "/api/v1/ws", true).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(status(None, "/api/v1/ws", true).await, StatusCode::OK);
}
