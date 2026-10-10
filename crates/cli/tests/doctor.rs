//! Offline `sentinal doctor` behavior tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT_TEST_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_TEST_DIR.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "sentinal-doctor-{}-{nonce}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
#[cfg(unix)]
fn doctor_reports_healthy_unpaired_install_without_connecting() {
    let root = TestDir::new();
    let prefix = root.0.join("install prefix");
    let config = root.0.join("config");
    let fake_bin = root.0.join("fake-bin");
    fs::create_dir_all(prefix.join("bin")).unwrap();
    fs::create_dir_all(prefix.join("share/sentinal")).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(&prefix.join("bin/sentinal"), "#!/bin/sh\nexit 0\n");
    write_executable(&prefix.join("bin/sentinald"), "#!/bin/sh\nexit 0\n");
    fs::write(
        prefix.join("share/sentinal/release.json"),
        "{\"schema_version\":1,\"version\":\"0.0.1\",\"target\":\"aarch64-apple-darwin\"}",
    )
    .unwrap();
    fs::write(
        config.join("daemon.toml"),
        "bind = \"100.90.80.70:7420\"\ndata_dir = \"/safe/data\"\n",
    )
    .unwrap();
    write_executable(
        &fake_bin.join("tailscale"),
        "#!/bin/sh\ncase \"$1 $2\" in\n  'status '|'status') echo connected ;;\n  'ip -4') echo 100.90.80.70 ;;\n  *) exit 2 ;;\nesac\n",
    );
    let service_command = if cfg!(target_os = "macos") {
        "launchctl"
    } else {
        "systemctl"
    };
    write_executable(&fake_bin.join(service_command), "#!/bin/sh\nexit 0\n");
    let output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args([
            "doctor",
            "--prefix",
            prefix.to_str().unwrap(),
            "--config-dir",
            config.to_str().unwrap(),
        ])
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Binaries: OK"));
    assert!(stdout.contains("Config: OK"));
    assert!(stdout.contains("Tailscale: OK (100.90.80.70)"));
    assert!(stdout.contains("Service: OK"));
    assert!(stdout.contains("Version: 0.0.1 (aarch64-apple-darwin)"));
    assert!(stdout.contains("Role: WORKER"));
    assert!(stdout.contains("Pairing: NOT CONNECTED"));
    assert!(stdout.contains("sentinal connect"));
    assert!(stdout.contains("Updates: none"));
    assert!(!stdout.contains("paired implicitly"));
}

#[test]
fn doctor_fails_clearly_when_worker_is_not_installed() {
    let root = TestDir::new();
    // Tailscale may really be installed (it is in /usr/bin on Linux hosts).
    let fake_bin = root.0.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(&fake_bin.join("tailscale"), "#!/bin/sh\nexit 1\n");
    let output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args([
            "doctor",
            "--prefix",
            root.0.join("missing").to_str().unwrap(),
            "--config-dir",
            root.0.join("missing-config").to_str().unwrap(),
        ])
        .env("PATH", format!("{}:/usr/bin:/bin", fake_bin.display()))
        .env("SENTINAL_TAILSCALE_CLI", fake_bin.join("tailscale"))
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Binaries: ERROR"));
    assert!(stdout.contains("Config: ERROR"));
    assert!(stdout.contains("Tailscale: ERROR"));
    assert!(stdout.contains("Pairing: NOT CONNECTED"));
}

#[test]
#[cfg(unix)]
fn doctor_uses_an_explicit_tailscale_cli_path() {
    let root = TestDir::new();
    let fixture = Fixture::new(&root, "bind = \"100.90.80.70:7420\"\n");
    fs::remove_file(fixture.fake_bin.join("tailscale")).unwrap();
    let app_cli = root.0.join("Tailscale.app/Contents/MacOS/Tailscale");
    fs::create_dir_all(app_cli.parent().unwrap()).unwrap();
    write_executable(
        &app_cli,
        "#!/bin/sh\ncase \"$1 $2\" in\n  'status '|'status') echo ok ;;\n  'ip -4') echo 100.90.80.71 ;;\n  *) exit 2 ;;\nesac\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args([
            "doctor",
            "--prefix",
            fixture.prefix.to_str().unwrap(),
            "--config-dir",
            fixture.config.to_str().unwrap(),
        ])
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", fixture.fake_bin.display()),
        )
        .env("SENTINAL_TAILSCALE_CLI", &app_cli)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Tailscale: OK (100.90.80.71)"), "{stdout}");
}

#[test]
#[cfg(unix)]
fn doctor_reports_the_real_link_without_printing_credentials() {
    let root = TestDir::new();
    let fixture = Fixture::new(&root, "bind = \"100.90.80.70:7420\"\n");
    fs::write(
        fixture.prefix.join("share/sentinal/release.json"),
        r#"{"schema_version":1,"version":"0.1.16","target":"x86_64-unknown-linux-musl","control_url":"https://control.example.ts.net"}"#,
    )
    .unwrap();
    fs::write(
        fixture.config.join("link.json"),
        r#"{"version":1,"state":"ACTIVE","control_url":"https://control.example.ts.net","display_name":"RTX PC","worker_credential":"planted-worker-credential","link_id":"planted-link-id"}"#,
    )
    .unwrap();
    let stdout = fixture.doctor();
    assert!(
        stdout.contains("Version: 0.1.16 (x86_64-unknown-linux-musl)"),
        "{stdout}"
    );
    assert!(stdout.contains("Role: WORKER"), "{stdout}");
    assert!(
        stdout.contains("Pairing: CONNECTED to https://control.example.ts.net as RTX PC"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Updates: https://control.example.ts.net (stable)"),
        "{stdout}"
    );
    assert!(!stdout.contains("planted-worker-credential"));
    assert!(!stdout.contains("planted-link-id"));

    fs::write(
        fixture.config.join("link.json"),
        r#"{"version":1,"state":"REVOKED","control_url":"https://control.example.ts.net"}"#,
    )
    .unwrap();
    assert!(fixture
        .doctor()
        .contains("Pairing: REVOKED by https://control.example.ts.net"));

    fs::write(
        fixture.config.join("link.json"),
        "not json planted-worker-credential",
    )
    .unwrap();
    let corrupt = fixture.doctor();
    assert!(corrupt.contains("Pairing: ERROR"), "{corrupt}");
    assert!(!corrupt.contains("planted-worker-credential"));
}

#[test]
#[cfg(unix)]
fn doctor_says_the_control_needs_no_pairing_and_updates_from_releases() {
    let root = TestDir::new();
    let fixture = Fixture::new(
        &root,
        "bind = \"127.0.0.1:7420\"\n[control]\nenabled = true\npublic_url = \"https://control.example.ts.net\"\n",
    );
    let stdout = fixture.doctor();
    assert!(
        stdout.contains("Role: CONTROL (https://control.example.ts.net)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Pairing: not needed on the control"),
        "{stdout}"
    );
    assert!(stdout.contains("Updates: just receive-release"), "{stdout}");
}

struct Fixture {
    prefix: PathBuf,
    config: PathBuf,
    fake_bin: PathBuf,
}

impl Fixture {
    fn new(root: &TestDir, daemon_toml: &str) -> Self {
        let prefix = root.0.join("install prefix");
        let config = root.0.join("config");
        let fake_bin = root.0.join("fake-bin");
        fs::create_dir_all(prefix.join("bin")).unwrap();
        fs::create_dir_all(prefix.join("share/sentinal")).unwrap();
        fs::create_dir_all(&config).unwrap();
        fs::create_dir_all(&fake_bin).unwrap();
        write_executable(&prefix.join("bin/sentinal"), "#!/bin/sh\nexit 0\n");
        write_executable(&prefix.join("bin/sentinald"), "#!/bin/sh\nexit 0\n");
        fs::write(config.join("daemon.toml"), daemon_toml).unwrap();
        write_executable(
            &fake_bin.join("tailscale"),
            "#!/bin/sh\ncase \"$1 $2\" in\n  'status '|'status') echo connected ;;\n  'ip -4') echo 100.90.80.70 ;;\n  *) exit 2 ;;\nesac\n",
        );
        for service in ["launchctl", "systemctl"] {
            write_executable(&fake_bin.join(service), "#!/bin/sh\nexit 0\n");
        }
        Self {
            prefix,
            config,
            fake_bin,
        }
    }

    fn doctor(&self) -> String {
        let output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
            .args([
                "doctor",
                "--prefix",
                self.prefix.to_str().unwrap(),
                "--config-dir",
                self.config.to_str().unwrap(),
            ])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.fake_bin.display(),
                    std::env::var("PATH").unwrap_or_default()
                ),
            )
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }
}

#[cfg(unix)]
fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
