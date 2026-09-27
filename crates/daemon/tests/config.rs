//! Configuration behaviour tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::config::{ConfigError, DaemonConfig};
use std::path::Path;

#[test]
fn defaults_bind_only_to_loopback() {
    let config = DaemonConfig::from_toml("", Path::new("/home/tester")).unwrap();

    assert_eq!(config.bind.to_string(), "127.0.0.1:7420");
    assert_eq!(config.scrollback_lines, 10_000);
    assert_eq!(
        config.data_dir,
        Path::new("/home/tester/.local/share/terminal-agent")
    );
    assert_eq!(
        config.token_file,
        Path::new("/home/tester/.config/terminal-agent/token")
    );
    assert_eq!(
        config.auth_file,
        Path::new("/home/tester/.config/terminal-agent/auth.json")
    );
}

#[test]
fn configured_values_are_loaded() {
    let text = r#"
bind = "127.0.0.1:9000"
data_dir = "/var/tmp/terminal-agent"
scrollback_lines = 25
token_file = "/var/tmp/terminal-agent.token"
auth_file = "/var/tmp/terminal-agent-auth.json"
"#;

    let config = DaemonConfig::from_toml(text, Path::new("/unused")).unwrap();

    assert_eq!(config.bind.to_string(), "127.0.0.1:9000");
    assert_eq!(config.scrollback_lines, 25);
    assert_eq!(config.data_dir, Path::new("/var/tmp/terminal-agent"));
    assert_eq!(
        config.auth_file,
        Path::new("/var/tmp/terminal-agent-auth.json")
    );
}

#[test]
fn ipv4_unspecified_bind_is_refused() {
    let error =
        DaemonConfig::from_toml("bind = \"0.0.0.0:7420\"", Path::new("/home/tester")).unwrap_err();

    assert!(matches!(error, ConfigError::UnsafeBind(ref address) if address == "0.0.0.0:7420"));
    assert!(error.to_string().contains("0.0.0.0:7420"));
}

#[test]
fn ipv6_unspecified_bind_is_refused() {
    let error =
        DaemonConfig::from_toml("bind = \"[::]:7420\"", Path::new("/home/tester")).unwrap_err();

    assert!(matches!(error, ConfigError::UnsafeBind(address) if address == "[::]:7420"));
}

#[test]
fn private_lan_bind_is_refused() {
    let error = DaemonConfig::from_toml("bind = \"192.168.1.20:7420\"", Path::new("/home/tester"))
        .unwrap_err();

    assert!(matches!(error, ConfigError::UnsafeBind(address) if address == "192.168.1.20:7420"));
}

#[test]
fn tailscale_ipv4_bind_is_allowed() {
    let config =
        DaemonConfig::from_toml("bind = \"100.100.10.20:7420\"", Path::new("/home/tester"))
            .unwrap();

    assert_eq!(config.bind.to_string(), "100.100.10.20:7420");
}

#[test]
fn tailscale_ipv6_bind_is_allowed() {
    let config = DaemonConfig::from_toml(
        "bind = \"[fd7a:115c:a1e0::1]:7420\"",
        Path::new("/home/tester"),
    )
    .unwrap();

    assert_eq!(config.bind.to_string(), "[fd7a:115c:a1e0::1]:7420");
}
