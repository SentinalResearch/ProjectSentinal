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
}

#[test]
fn configured_values_are_loaded() {
    let text = r#"
bind = "127.0.0.1:9000"
data_dir = "/var/tmp/terminal-agent"
scrollback_lines = 25
token_file = "/var/tmp/terminal-agent.token"
"#;

    let config = DaemonConfig::from_toml(text, Path::new("/unused")).unwrap();

    assert_eq!(config.bind.to_string(), "127.0.0.1:9000");
    assert_eq!(config.scrollback_lines, 25);
    assert_eq!(config.data_dir, Path::new("/var/tmp/terminal-agent"));
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
