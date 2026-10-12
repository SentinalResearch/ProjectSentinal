//! Owner-signed releases: the package service serves SHA256SUMS and its
//! signature, and the generated install.sh verifies them with the pinned keys
//! before anything is installed. Real `ssh-keygen`, throwaway keys, no network.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use daemon::{
    config::PackageServiceConfig,
    package_service::{
        package_router, render_install_script, PackageService, ReleaseManifest, ReleaseTarget,
        TargetArch, TargetOs, PINNED_RELEASE_KEYS,
    },
};
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sentinal-signed-{name}-{nonce}"));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A throwaway Ed25519 key; returns its private-key path and allowed_signers line.
fn release_key(dir: &Path, name: &str) -> (PathBuf, String) {
    let key = dir.join(name);
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", name, "-f"])
        .arg(&key)
        .status()
        .unwrap();
    assert!(status.success());
    let public = fs::read_to_string(key.with_extension("pub")).unwrap();
    let mut fields = public.split_whitespace();
    let line = format!(
        "sentinal-release namespaces=\"sentinal-release\" {} {}\n",
        fields.next().unwrap(),
        fields.next().unwrap()
    );
    (key, line)
}

fn sign(key: &Path, sums: &Path) {
    let _ = fs::remove_file(sums.with_extension("sig"));
    let status = Command::new("ssh-keygen")
        .args(["-q", "-Y", "sign", "-n", "sentinal-release", "-f"])
        .arg(key)
        .arg(sums)
        .status()
        .unwrap();
    assert!(status.success());
}

fn sha256(path: &Path) -> String {
    let output = Command::new("sh")
        .arg("-c")
        .arg("if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$1\"; else shasum -a 256 \"$1\"; fi")
        .arg("sha")
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn host_target() -> (&'static str, TargetOs, TargetArch) {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "x86_64") => ("x86_64-apple-darwin", TargetOs::Macos, TargetArch::X86_64),
        ("macos", "aarch64") => ("aarch64-apple-darwin", TargetOs::Macos, TargetArch::Aarch64),
        ("linux", "x86_64") => (
            "x86_64-unknown-linux-musl",
            TargetOs::Linux,
            TargetArch::X86_64,
        ),
        ("linux", "aarch64") => (
            "aarch64-unknown-linux-musl",
            TargetOs::Linux,
            TargetArch::Aarch64,
        ),
        other => panic!("unsupported test host: {other:?}"),
    }
}

fn config(root: &Path, extra_release_keys: Option<PathBuf>) -> PackageServiceConfig {
    PackageServiceConfig {
        listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7422),
        directory: root.to_path_buf(),
        public_base_url: "https://sentinal-control.example.ts.net".into(),
        max_artifact_bytes: 1024 * 1024,
        web_root: None,
        extra_release_keys,
    }
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    (
        status,
        to_bytes(response.into_body(), 1 << 20)
            .await
            .unwrap()
            .to_vec(),
    )
}

#[test]
fn compiled_release_keys_are_the_repository_pin() {
    let pinned = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/release-keys/allowed_signers"),
    )
    .unwrap();
    assert_eq!(PINNED_RELEASE_KEYS, pinned);
}

#[tokio::test]
async fn serves_signed_sums_beside_each_release_and_refuses_other_names() {
    let root = TestDir::new("routes");
    let packages = root.0.join("packages");
    for child in ["channels", "releases", "artifacts"] {
        fs::create_dir_all(packages.join(child)).unwrap();
    }
    fs::set_permissions(&packages, fs::Permissions::from_mode(0o700)).unwrap();
    let sums = format!(
        "{}  sentinal-worker-0.0.1-x86_64-apple-darwin.tar.gz\n",
        "a".repeat(64)
    );
    fs::write(packages.join("releases/0.0.1.SHA256SUMS"), &sums).unwrap();
    fs::write(
        packages.join("releases/0.0.1.SHA256SUMS.sig"),
        b"-----BEGIN SSH SIGNATURE-----\n",
    )
    .unwrap();
    let app = package_router(
        PackageService::open(
            config(&packages, None),
            fs::metadata(&packages).unwrap().uid(),
        )
        .unwrap(),
    );

    let (status, body) = get(&app, "/packages/v1/releases/0.0.1.SHA256SUMS").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, sums.as_bytes());
    let (status, body) = get(&app, "/packages/v1/releases/0.0.1.SHA256SUMS.sig").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.starts_with(b"-----BEGIN SSH SIGNATURE"));

    assert_eq!(
        get(&app, "/packages/v1/releases/0.0.2.SHA256SUMS").await.0,
        StatusCode::NOT_FOUND
    );
    for bad in [
        "0.0.1.SHA256SUMS.bak",
        "0.0.1.sig",
        "0.0.1..SHA256SUMS",
        "x.SHA256SUMS",
    ] {
        assert_eq!(
            get(&app, &format!("/packages/v1/releases/{bad}")).await.0,
            StatusCode::BAD_REQUEST,
            "{bad} must be refused"
        );
    }
}

#[test]
fn extra_release_keys_must_be_owner_only_well_formed_key_lines() {
    let root = TestDir::new("extra");
    let packages = root.0.join("packages");
    fs::create_dir_all(&packages).unwrap();
    fs::set_permissions(&packages, fs::Permissions::from_mode(0o700)).unwrap();
    let uid = fs::metadata(&packages).unwrap().uid();
    let (_, line) = release_key(&root.0, "dev");
    let extra = root.0.join("extra_release_keys");

    fs::write(&extra, format!("# development key\n{line}")).unwrap();
    fs::set_permissions(&extra, fs::Permissions::from_mode(0o600)).unwrap();
    let manifest = signed_manifest_for_test();
    let opened = PackageService::open(config(&packages, Some(extra.clone())), uid).unwrap();
    assert!(opened.release_keys().contains(line.trim()));

    fs::set_permissions(&extra, fs::Permissions::from_mode(0o666)).unwrap();
    let error = PackageService::open(config(&packages, Some(extra.clone())), uid).unwrap_err();
    assert!(error.to_string().contains("release keys"), "{error}");

    fs::set_permissions(&extra, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&extra, "ssh-ed25519 AAAA not-a-sentinal-release-line\n").unwrap();
    let error = PackageService::open(config(&packages, Some(extra)), uid).unwrap_err();
    assert!(error.to_string().contains("release keys"), "{error}");
    drop(manifest);
}

fn signed_manifest_for_test() -> ReleaseManifest {
    let (target, os, arch) = host_target();
    ReleaseManifest {
        schema_version: 1,
        channel: "stable".into(),
        release_version: "0.0.1".into(),
        published_at: 1_790_712_000_000,
        minimum_protocol_version: 1,
        maximum_protocol_version: 1,
        targets: vec![ReleaseTarget {
            target: target.into(),
            os,
            arch,
            artifact_path: format!("/packages/v1/artifacts/sentinal-worker-0.0.1-{target}.tar.gz"),
            bytes: 1,
            sha256: "0".repeat(64),
        }],
    }
}

struct Bootstrap {
    root: TestDir,
    archive: PathBuf,
    sums: PathBuf,
    manifest: ReleaseManifest,
    path: String,
}

impl Bootstrap {
    fn new() -> Self {
        let root = TestDir::new("bootstrap");
        let release = root.0.join("release/sentinal-worker");
        fs::create_dir_all(release.join("bin")).unwrap();
        fs::create_dir_all(release.join("scripts")).unwrap();
        write_executable(
            &release.join("bin/sentinal"),
            "#!/bin/sh\nprintf '{\"state\": \"NOT_CONNECTED\"}\\n'\n",
        );
        write_executable(&release.join("bin/sentinald"), "#!/bin/sh\nexit 0\n");
        write_executable(
            &release.join("scripts/install-worker.sh"),
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >\"$SENTINAL_TEST_INSTALL_LOG\"\n",
        );
        let (target, os, arch) = host_target();
        let name = format!("sentinal-worker-0.0.1-{target}.tar.gz");
        let archive = root.0.join(&name);
        assert!(Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(root.0.join("release"))
            .arg("sentinal-worker")
            .status()
            .unwrap()
            .success());
        let digest = sha256(&archive);
        let sums = root.0.join("SHA256SUMS");
        fs::write(&sums, format!("{digest}  {name}\n")).unwrap();
        let manifest = ReleaseManifest {
            schema_version: 1,
            channel: "stable".into(),
            release_version: "0.0.1".into(),
            published_at: 1_790_712_000_000,
            minimum_protocol_version: 1,
            maximum_protocol_version: 1,
            targets: vec![ReleaseTarget {
                target: target.into(),
                os,
                arch,
                artifact_path: format!("/packages/v1/artifacts/{name}"),
                bytes: fs::metadata(&archive).unwrap().len(),
                sha256: digest,
            }],
        };
        let fake_bin = root.0.join("fake-bin");
        fs::create_dir_all(&fake_bin).unwrap();
        write_executable(
            &fake_bin.join("curl"),
            r#"#!/bin/sh
set -eu
output=''
url=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output="$2"; shift 2 ;;
    https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
printf '%s\n' "$url" >>"$SENTINAL_TEST_CURL_LOG"
case "$url" in
  */packages/v1/releases/0.0.1.SHA256SUMS.sig) source="$SENTINAL_TEST_SUMS.sig" ;;
  */packages/v1/releases/0.0.1.SHA256SUMS) source="$SENTINAL_TEST_SUMS" ;;
  */packages/v1/artifacts/*) source="$SENTINAL_TEST_ARCHIVE" ;;
  *) exit 22 ;;
esac
[ -f "$source" ] || exit 22
cp "$source" "$output"
"#,
        );
        write_executable(
            &fake_bin.join("tailscale"),
            "#!/bin/sh\n[ \"$1 $2\" = 'ip -4' ] || exit 2\nprintf '%s\\n' '100.90.80.70'\n",
        );
        let path = format!(
            "{}:{}",
            fake_bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        Self {
            root,
            archive,
            sums,
            manifest,
            path,
        }
    }

    /// Run install.sh generated with `keys`; returns (success, stderr, installer ran).
    fn run(&self, keys: &str) -> (bool, String, bool) {
        let script = self.root.0.join("install.sh");
        write_executable(
            &script,
            &render_install_script(
                "https://sentinal-control.example.ts.net",
                &self.manifest,
                keys,
            ),
        );
        let log = self.root.0.join("installer.log");
        let _ = fs::remove_file(&log);
        let output = Command::new("/bin/sh")
            .arg(&script)
            .env("PATH", &self.path)
            .env("HOME", self.root.0.join("home"))
            .env("SENTINAL_TEST_ARCHIVE", &self.archive)
            .env("SENTINAL_TEST_SUMS", &self.sums)
            .env("SENTINAL_TEST_INSTALL_LOG", &log)
            .env("SENTINAL_TEST_CURL_LOG", self.root.0.join("curl.log"))
            .output()
            .unwrap();
        (
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
            log.exists(),
        )
    }
}

#[test]
fn install_script_installs_only_owner_signed_releases() {
    let run = Bootstrap::new();
    let (key, pinned) = release_key(&run.root.0, "pinned");
    let (stranger, _) = release_key(&run.root.0, "stranger");
    sign(&key, &run.sums);

    let (ok, stderr, installed) = run.run(&pinned);
    assert!(
        ok && installed,
        "a correctly signed release failed: {stderr}"
    );

    // No pinned key: fail closed before downloading anything.
    let (ok, stderr, installed) = run.run("# no keys\n");
    assert!(!ok && !installed);
    assert!(
        stderr.contains("no pinned release signing keys"),
        "{stderr}"
    );

    // A signature from a key that is not pinned.
    sign(&stranger, &run.sums);
    let (ok, stderr, installed) = run.run(&pinned);
    assert!(!ok && !installed);
    assert!(
        stderr.contains("not signed by a trusted Sentinal release key"),
        "{stderr}"
    );

    // A missing signature.
    fs::remove_file(run.sums.with_extension("sig")).unwrap();
    let (ok, stderr, installed) = run.run(&pinned);
    assert!(!ok && !installed);
    assert!(stderr.contains("not signed"), "{stderr}");

    // SHA256SUMS changed after signing.
    sign(&key, &run.sums);
    let original = fs::read_to_string(&run.sums).unwrap();
    fs::write(
        &run.sums,
        format!("{original}{}  extra.tar.gz\n", "b".repeat(64)),
    )
    .unwrap();
    let (ok, stderr, installed) = run.run(&pinned);
    assert!(!ok && !installed);
    assert!(
        stderr.contains("not signed by a trusted Sentinal release key"),
        "{stderr}"
    );

    // Signed, but the control's manifest names a different digest than the signed list.
    let name = run.manifest.targets[0]
        .artifact_path
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    fs::write(&run.sums, format!("{}  {name}\n", "c".repeat(64))).unwrap();
    sign(&key, &run.sums);
    let (ok, stderr, installed) = run.run(&pinned);
    assert!(!ok && !installed);
    assert!(
        stderr.contains("does not match the signed SHA256SUMS"),
        "{stderr}"
    );
}
