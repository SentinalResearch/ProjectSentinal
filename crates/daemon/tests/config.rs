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
    assert_eq!(
        config.local_socket,
        Path::new("/home/tester/.local/share/terminal-agent/run/sentinald.sock")
    );
    assert!(config.packages.is_none());
}

#[test]
fn configured_values_are_loaded() {
    let text = r#"
bind = "127.0.0.1:9000"
data_dir = "/var/tmp/terminal-agent"
scrollback_lines = 25
token_file = "/var/tmp/terminal-agent.token"
auth_file = "/var/tmp/terminal-agent-auth.json"
local_socket = "/var/tmp/sentinald.sock"
"#;

    let config = DaemonConfig::from_toml(text, Path::new("/unused")).unwrap();

    assert_eq!(config.bind.to_string(), "127.0.0.1:9000");
    assert_eq!(config.scrollback_lines, 25);
    assert_eq!(config.data_dir, Path::new("/var/tmp/terminal-agent"));
    assert_eq!(
        config.auth_file,
        Path::new("/var/tmp/terminal-agent-auth.json")
    );
    assert_eq!(config.local_socket, Path::new("/var/tmp/sentinald.sock"));
}

#[test]
fn linux_runtime_directory_is_preferred_when_injected() {
    let config = DaemonConfig::from_toml_with_runtime(
        "",
        Path::new("/home/tester"),
        Some(Path::new("/run/user/1000")),
    )
    .unwrap();
    let expected = if cfg!(target_os = "macos") {
        Path::new("/home/tester/.local/share/terminal-agent/run/sentinald.sock")
    } else {
        Path::new("/run/user/1000/sentinal/sentinald.sock")
    };
    assert_eq!(config.local_socket, expected);
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

#[test]
fn package_service_loads_only_on_loopback_https_tailnet_name() {
    let config = DaemonConfig::from_toml(
        r#"
[packages]
enabled = true
listen = "127.0.0.1:7422"
directory = "/srv/sentinal/packages"
public_base_url = "https://sentinal-control.example.ts.net"
max_artifact_bytes = 1048576
"#,
        Path::new("/home/tester"),
    )
    .unwrap();

    let packages = config.packages.unwrap();
    assert_eq!(packages.listen.to_string(), "127.0.0.1:7422");
    assert_eq!(packages.directory, Path::new("/srv/sentinal/packages"));
    assert_eq!(
        packages.public_base_url,
        "https://sentinal-control.example.ts.net"
    );
    assert_eq!(packages.max_artifact_bytes, 1_048_576);
}

#[test]
fn package_service_refuses_non_loopback_listener() {
    let error = DaemonConfig::from_toml(
        r#"
[packages]
enabled = true
listen = "100.100.10.20:7422"
directory = "/srv/sentinal/packages"
public_base_url = "https://sentinal-control.example.ts.net"
"#,
        Path::new("/home/tester"),
    )
    .unwrap_err();

    assert!(
        matches!(error, ConfigError::UnsafePackageBind(ref value) if value == "100.100.10.20:7422")
    );
}

#[test]
fn package_service_refuses_plain_http_and_non_tailnet_names() {
    for url in [
        "http://sentinal-control.example.ts.net",
        "https://192.168.1.20",
        "https://sentinal.example.com",
        "https://user:secret@sentinal-control.example.ts.net",
        "https://sentinal-control.example.ts.net/path",
    ] {
        let text = format!(
            r#"
[packages]
enabled = true
directory = "/srv/sentinal/packages"
public_base_url = "{url}"
"#
        );
        let error = DaemonConfig::from_toml(&text, Path::new("/home/tester")).unwrap_err();
        assert!(
            matches!(error, ConfigError::InvalidPackageUrl { .. }),
            "{url}: {error}"
        );
    }
}

#[test]
fn disabled_package_section_does_not_require_service_fields() {
    let config =
        DaemonConfig::from_toml("[packages]\nenabled = false\n", Path::new("/home/tester"))
            .unwrap();

    assert!(config.packages.is_none());
}
