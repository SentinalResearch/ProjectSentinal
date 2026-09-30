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
    assert!(stdout.contains("Pairing: UNPAIRED"));
    assert!(!stdout.contains("paired implicitly"));
}

#[test]
fn doctor_fails_clearly_when_worker_is_not_installed() {
    let root = TestDir::new();
    let output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args([
            "doctor",
            "--prefix",
            root.0.join("missing").to_str().unwrap(),
            "--config-dir",
            root.0.join("missing-config").to_str().unwrap(),
        ])
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();

    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Binaries: ERROR"));
    assert!(stdout.contains("Config: ERROR"));
    assert!(stdout.contains("Tailscale: ERROR"));
    assert!(stdout.contains("Pairing: UNPAIRED"));
}

#[cfg(unix)]
fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
