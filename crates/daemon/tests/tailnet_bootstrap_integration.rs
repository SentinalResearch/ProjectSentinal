//! Manual real-Tailnet package installation acceptance.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{fs, path::PathBuf, process::Command, time::SystemTime};

#[test]
#[ignore = "requires a published real Tailnet release and intentionally starts a user service"]
fn tailnet_bootstrap_integration() {
    assert_eq!(
        std::env::var("SENTINAL_BOOTSTRAP_CONFIRM").as_deref(),
        Ok("install-unpaired-worker"),
        "set SENTINAL_BOOTSTRAP_CONFIRM=install-unpaired-worker to acknowledge the real install"
    );
    let control_url = std::env::var("SENTINAL_BOOTSTRAP_URL")
        .expect("set SENTINAL_BOOTSTRAP_URL to the control Machine's HTTPS .ts.net origin");
    assert!(control_url.starts_with("https://") && control_url.ends_with(".ts.net"));
    let nonce = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let home = std::env::temp_dir().join(format!("sentinal-bootstrap-integration-{nonce}"));
    let prefix = home.join(".local");
    fs::create_dir_all(&home).unwrap();
    let script = home.join("install.sh");
    let download = Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--max-redirs",
            "0",
            "--fail",
            "--silent",
            "--show-error",
            "--output",
        ])
        .arg(&script)
        .arg(format!("{control_url}/install.sh"))
        .status()
        .unwrap();
    assert!(download.success());
    let install = Command::new("sh")
        .arg(&script)
        .arg("--prefix")
        .arg(&prefix)
        .env("HOME", &home)
        .status()
        .unwrap();
    assert!(install.success());

    let doctor = Command::new(prefix.join("bin/sentinal"))
        .args(["doctor", "--prefix"])
        .arg(&prefix)
        .arg("--config-dir")
        .arg(home.join(".config/terminal-agent"))
        .env("HOME", &home)
        .output()
        .unwrap();
    let report = String::from_utf8_lossy(&doctor.stdout);
    assert!(report.contains("Pairing: UNPAIRED"), "{report}");

    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let data = home.join(".local/share/terminal-agent");
    let uninstall = Command::new(repository.join("scripts/install-worker.sh"))
        .arg("uninstall")
        .arg("--prefix")
        .arg(&prefix)
        .arg("--config-dir")
        .arg(home.join(".config/terminal-agent"))
        .arg("--data-dir")
        .arg(&data)
        .env("HOME", &home)
        .status()
        .unwrap();
    assert!(uninstall.success());
    assert!(data.exists(), "uninstall must retain worker data");
    fs::remove_dir_all(home).unwrap();
}
