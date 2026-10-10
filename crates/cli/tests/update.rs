//! Verified update-command behavior tests.

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
            "sentinal-update-{}-{nonce}-{sequence}",
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
fn update_reuses_control_install_script_for_check_install_and_rollback() {
    let root = TestDir::new();
    let prefix = root.0.join("prefix with spaces");
    let state_dir = prefix.join("share/sentinal");
    let fake_bin = root.0.join("fake-bin");
    fs::create_dir_all(&state_dir).unwrap();
    fs::create_dir_all(&fake_bin).unwrap();
    fs::write(
        state_dir.join("release.json"),
        r#"{"schema_version":1,"version":"0.0.1","control_url":"https://sentinal-control.example.ts.net"}"#,
    )
    .unwrap();
    let served_script = root.0.join("served-install.sh");
    fs::write(
        &served_script,
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >\"$SENTINAL_UPDATE_ARGS\"\n",
    )
    .unwrap();
    let curl_log = root.0.join("curl.log");
    write_executable(
        &fake_bin.join("curl"),
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >"$SENTINAL_CURL_LOG"
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '--output' ]; then output="$2"; shift 2; else shift; fi
done
cp "$SENTINAL_SERVED_SCRIPT" "$output"
"#,
    );
    let update_args = root.0.join("update.args");
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    for args in [
        vec!["update", "--check"],
        vec!["update"],
        vec!["update", "--rollback", "0.0.1"],
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sentinal"));
        command
            .args(args)
            .args(["--prefix", prefix.to_str().unwrap()]);
        let output = command
            .env("PATH", &path)
            .env("SENTINAL_SERVED_SCRIPT", &served_script)
            .env("SENTINAL_CURL_LOG", &curl_log)
            .env("SENTINAL_UPDATE_ARGS", &update_args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let curl = fs::read_to_string(&curl_log).unwrap();
        assert!(curl.contains("--proto =https"));
        assert!(curl.contains("--max-filesize 65536"));
        assert!(curl.contains("https://sentinal-control.example.ts.net/install.sh"));
        assert!(!curl.contains("token"));
    }
    assert_eq!(
        fs::read_to_string(&update_args).unwrap().trim(),
        format!("--rollback 0.0.1 --prefix {}", prefix.display())
    );
}

#[test]
#[cfg(unix)]
fn update_rejects_unsafe_origin_and_oversized_script() {
    let root = TestDir::new();
    let prefix = root.0.join("prefix");
    fs::create_dir_all(prefix.join("share/sentinal")).unwrap();
    fs::write(
        prefix.join("share/sentinal/release.json"),
        r#"{"schema_version":1,"version":"0.0.1","control_url":"http://public.example.com"}"#,
    )
    .unwrap();
    let unsafe_output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args(["update", "--prefix", prefix.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!unsafe_output.status.success());
    assert!(String::from_utf8_lossy(&unsafe_output.stderr).contains("HTTPS .ts.net"));

    fs::write(
        prefix.join("share/sentinal/release.json"),
        r#"{"schema_version":1,"version":"0.0.1","control_url":"https://sentinal-control.example.ts.net"}"#,
    )
    .unwrap();
    let fake_bin = root.0.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(
        &fake_bin.join("curl"),
        "#!/bin/sh\noutput=''\nwhile [ \"$#\" -gt 0 ]; do if [ \"$1\" = --output ]; then output=\"$2\"; shift 2; else shift; fi; done\ndd if=/dev/zero of=\"$output\" bs=65537 count=1 2>/dev/null\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args(["update", "--prefix", prefix.to_str().unwrap()])
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
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exceeds 65536 bytes"));
}

#[test]
#[cfg(unix)]
fn update_names_the_real_version_and_explains_a_missing_origin() {
    let root = TestDir::new();
    let prefix = root.0.join("prefix");
    fs::create_dir_all(prefix.join("share/sentinal")).unwrap();
    fs::write(
        prefix.join("share/sentinal/release.json"),
        r#"{"schema_version":1,"version":"0.1.15","target":"x86_64-unknown-linux-musl"}"#,
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args(["update", "--check", "--prefix", prefix.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("sentinal 0.0.1 (development build)"),
        "{stdout}"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("has no update origin"), "{stderr}");
    assert!(stderr.contains("just receive-release"), "{stderr}");
    assert!(stderr.contains("install.sh"), "{stderr}");

    let version = Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&version.stdout).trim(),
        "sentinal 0.0.1 (development build)"
    );
}

#[cfg(unix)]
fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
