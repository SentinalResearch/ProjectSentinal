//! Daemon URL discovery for an installed Tailscale worker.

#![allow(clippy::unwrap_used)]

use cli::daemon_url::resolve;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_file(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sentinal-cli-url-{}-{}-{name}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn explicit_url_wins_even_when_the_local_config_is_bad() {
    let path = temp_file("bad.toml");
    fs::write(&path, "not = [valid").unwrap();

    let result = resolve(Some("ws://100.70.1.2:9000/custom"), &path).unwrap();

    assert_eq!(result, "ws://100.70.1.2:9000/custom");
    fs::remove_file(path).unwrap();
}

#[test]
fn installed_daemon_bind_becomes_the_default_websocket_url() {
    let path = temp_file("daemon.toml");
    fs::write(
        &path,
        "bind = \"100.90.80.70:17420\"\ndata_dir = \"/tmp/sentinal\"\n",
    )
    .unwrap();

    let result = resolve(None, &path).unwrap();

    assert_eq!(result, "ws://100.90.80.70:17420/api/v1/ws");
    fs::remove_file(path).unwrap();
}

#[test]
fn a_missing_config_keeps_the_loopback_development_default() {
    let path = temp_file("missing.toml");

    assert_eq!(
        resolve(None, &path).unwrap(),
        "ws://127.0.0.1:7420/api/v1/ws"
    );
}

#[test]
fn malformed_config_and_bind_are_named_errors() {
    let malformed = temp_file("malformed.toml");
    fs::write(&malformed, "bind = [").unwrap();
    let error = resolve(None, &malformed).unwrap_err().to_string();
    assert!(error.contains(&malformed.display().to_string()), "{error}");
    fs::remove_file(malformed).unwrap();

    let bad_bind = temp_file("bad-bind.toml");
    fs::write(&bad_bind, "bind = \"localhost:7420\"\n").unwrap();
    let error = resolve(None, &bad_bind).unwrap_err().to_string();
    assert!(error.contains("localhost:7420"), "{error}");
    fs::remove_file(bad_bind).unwrap();
}

#[test]
fn candidates_add_a_loopback_fallback_on_the_configured_port() {
    let config = temp_file("candidates.toml");
    std::fs::write(&config, "bind = \"100.74.239.89:7444\"\n").unwrap();
    assert_eq!(
        cli::daemon_url::candidates(None, &config).unwrap(),
        vec![
            "ws://100.74.239.89:7444/api/v1/ws".to_string(),
            "ws://127.0.0.1:7444/api/v1/ws".to_string()
        ]
    );
    std::fs::write(&config, "bind = \"127.0.0.1:7420\"\n").unwrap();
    assert_eq!(
        cli::daemon_url::candidates(None, &config).unwrap(),
        vec!["ws://127.0.0.1:7420/api/v1/ws".to_string()]
    );
    assert_eq!(
        cli::daemon_url::candidates(Some("ws://example.test/api/v1/ws"), &config).unwrap(),
        vec!["ws://example.test/api/v1/ws".to_string()]
    );
    fs::remove_file(config).unwrap();
}
