//! Private package-service behavior tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use daemon::{
    config::PackageServiceConfig,
    package_service::{
        package_router, render_install_script, PackageService, ReleaseManifest, ReleaseTarget,
        TargetArch, TargetOs,
    },
};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sentinal-package-{name}-{nonce}"));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn manifest_json(bytes: u64) -> String {
    format!(
        r#"{{
  "schema_version": 1,
  "channel": "stable",
  "release_version": "0.0.1",
  "published_at": 1790712000000,
  "minimum_protocol_version": 1,
  "maximum_protocol_version": 1,
  "targets": [{{
    "target": "x86_64-apple-darwin",
    "os": "MACOS",
    "arch": "X86_64",
    "artifact_path": "/packages/v1/artifacts/sentinal-worker-0.0.1-x86_64-apple-darwin.tar.gz",
    "bytes": {bytes},
    "sha256": "{SHA}"
  }}]
}}"#
    )
}

fn service(root: &Path) -> PackageService {
    PackageService::open(
        PackageServiceConfig {
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7422),
            directory: root.to_path_buf(),
            public_base_url: "https://sentinal-control.example.ts.net".into(),
            max_artifact_bytes: 1024 * 1024,
            web_root: None,
        },
        owner_uid(root),
    )
    .unwrap()
}

fn publish_fixture(root: &Path, artifact: &[u8]) {
    fs::create_dir_all(root.join("channels")).unwrap();
    fs::create_dir_all(root.join("releases")).unwrap();
    fs::create_dir_all(root.join("artifacts")).unwrap();
    let manifest = manifest_json(artifact.len() as u64);
    fs::write(root.join("channels/stable.json"), &manifest).unwrap();
    fs::write(root.join("releases/0.0.1.json"), &manifest).unwrap();
    fs::write(
        root.join("artifacts/sentinal-worker-0.0.1-x86_64-apple-darwin.tar.gz"),
        artifact,
    )
    .unwrap();
}

#[test]
fn manifest_accepts_supported_target_and_rejects_duplicate_target() {
    let manifest = ReleaseManifest::parse(manifest_json(12).as_bytes(), 1024 * 1024).unwrap();
    assert_eq!(manifest.release_version, "0.0.1");
    assert_eq!(manifest.targets.len(), 1);

    let duplicated = manifest_json(12).replace(
        "]\n}",
        &format!(
            ",{{\"target\":\"x86_64-apple-darwin\",\"os\":\"MACOS\",\"arch\":\"X86_64\",\"artifact_path\":\"/packages/v1/artifacts/sentinal-worker-0.0.1-x86_64-apple-darwin.tar.gz\",\"bytes\":12,\"sha256\":\"{SHA}\"}}]\n}}"
        ),
    );
    let error = ReleaseManifest::parse(duplicated.as_bytes(), 1024 * 1024).unwrap_err();
    assert!(error.to_string().contains("duplicate target"));
}

#[test]
fn manifest_rejects_incompatible_protocol_excessive_size_and_bad_checksum() {
    let incompatible = manifest_json(12).replace(
        "\"minimum_protocol_version\": 1",
        "\"minimum_protocol_version\": 2",
    );
    assert!(ReleaseManifest::parse(incompatible.as_bytes(), 1024 * 1024)
        .unwrap_err()
        .to_string()
        .contains("protocol"));

    let excessive = manifest_json(2_000_000);
    assert!(ReleaseManifest::parse(excessive.as_bytes(), 1024)
        .unwrap_err()
        .to_string()
        .contains("exceeds"));

    let bad_checksum = manifest_json(12).replace(SHA, "not-a-checksum");
    assert!(ReleaseManifest::parse(bad_checksum.as_bytes(), 1024 * 1024)
        .unwrap_err()
        .to_string()
        .contains("SHA-256"));
}

#[test]
fn manifest_accepts_every_supported_target_and_rejects_missing_or_bad_release_fields() {
    let targets = [
        (
            "x86_64-unknown-linux-musl",
            TargetOs::Linux,
            TargetArch::X86_64,
        ),
        (
            "aarch64-unknown-linux-musl",
            TargetOs::Linux,
            TargetArch::Aarch64,
        ),
        ("x86_64-apple-darwin", TargetOs::Macos, TargetArch::X86_64),
        ("aarch64-apple-darwin", TargetOs::Macos, TargetArch::Aarch64),
    ]
    .into_iter()
    .map(|(target, os, arch)| ReleaseTarget {
        target: target.into(),
        os,
        arch,
        artifact_path: format!("/packages/v1/artifacts/sentinal-worker-1.2.3-{target}.tar.gz"),
        bytes: 12,
        sha256: SHA.into(),
    })
    .collect();
    let manifest = ReleaseManifest {
        schema_version: 1,
        channel: "stable".into(),
        release_version: "1.2.3".into(),
        published_at: 1,
        minimum_protocol_version: 1,
        maximum_protocol_version: 1,
        targets,
    };
    manifest.validate(1024).unwrap();

    let missing = manifest_json(12).replace("  \"channel\": \"stable\",\n", "");
    assert!(ReleaseManifest::parse(missing.as_bytes(), 1024).is_err());
    let bad_channel = manifest_json(12).replace("\"stable\"", "\"nightly\"");
    assert!(ReleaseManifest::parse(bad_channel.as_bytes(), 1024)
        .unwrap_err()
        .to_string()
        .contains("channel"));
    let bad_version = manifest_json(12).replace("\"0.0.1\"", "\"01.0.1\"");
    assert!(ReleaseManifest::parse(bad_version.as_bytes(), 1024)
        .unwrap_err()
        .to_string()
        .contains("release_version"));
}

#[tokio::test]
async fn serves_manifest_install_script_and_exact_immutable_artifact() {
    let root = TestDir::new("routes");
    publish_fixture(&root.0, b"verified archive");
    let app = package_router(service(&root.0));

    let manifest_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/packages/v1/channels/stable.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(manifest_response.status(), StatusCode::OK);
    assert_eq!(
        manifest_response.headers()["content-type"],
        "application/json"
    );

    let script_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/install.sh")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(script_response.status(), StatusCode::OK);
    let script = to_bytes(script_response.into_body(), 65_536).await.unwrap();
    let script = String::from_utf8(script.to_vec()).unwrap();
    assert!(script.contains("https://sentinal-control.example.ts.net"));
    assert!(script.contains(SHA));
    assert!(script.contains("State: UNPAIRED"));
    assert!(script.contains("Settings → Machines → Add Machine"));
    assert!(
        !script.contains("--code"),
        "installation never pairs or carries a code"
    );
    assert!(!script.contains("token"));
    assert!(!script.contains("password"));

    let artifact_response = app
        .oneshot(
            Request::builder()
                .uri("/packages/v1/artifacts/sentinal-worker-0.0.1-x86_64-apple-darwin.tar.gz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(artifact_response.status(), StatusCode::OK);
    let body = to_bytes(artifact_response.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], b"verified archive");
}

#[tokio::test]
async fn rejects_ranges_traversal_unknown_releases_and_length_mismatch() {
    let root = TestDir::new("reject");
    publish_fixture(&root.0, b"verified archive");
    let app = package_router(service(&root.0));

    let ranged = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/packages/v1/artifacts/sentinal-worker-0.0.1-x86_64-apple-darwin.tar.gz")
                .header("range", "bytes=0-2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ranged.status(), StatusCode::RANGE_NOT_SATISFIABLE);

    for uri in [
        "/packages/v1/artifacts/%2e%2e%2fsecret",
        "/packages/v1/releases/9.9.9.json",
        "/packages/v1/artifacts/not-declared.tar.gz",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert!(
            matches!(
                response.status(),
                StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND
            ),
            "{uri}: {}",
            response.status()
        );
    }

    fs::write(
        root.0
            .join("artifacts/sentinal-worker-0.0.1-x86_64-apple-darwin.tar.gz"),
        b"short",
    )
    .unwrap();
    let mismatch = app
        .oneshot(
            Request::builder()
                .uri("/packages/v1/artifacts/sentinal-worker-0.0.1-x86_64-apple-darwin.tar.gz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatch.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn package_audit_is_metadata_only() {
    let root = TestDir::new("audit");
    publish_fixture(&root.0, b"terminal output must not appear in audit");
    let app = package_router(service(&root.0));
    let response = app
        .oneshot(
            Request::builder()
                .uri("/packages/v1/channels/stable.json?token=secret")
                .header("authorization", "Bearer secret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let audit = fs::read_to_string(root.0.join("package-audit.jsonl")).unwrap();
    assert!(audit.contains("CHANNEL"));
    assert!(!audit.contains("secret"));
    assert!(!audit.contains("terminal output"));
    assert!(!audit.contains("authorization"));
}

#[tokio::test]
async fn production_web_shell_has_safe_fallback_headers_and_api_precedence() {
    let root = TestDir::new("web-shell");
    let packages = root.0.join("packages");
    let web = root.0.join("web");
    publish_fixture(&packages, b"verified archive");
    fs::create_dir_all(web.join("assets")).unwrap();
    fs::write(
        web.join("index.html"),
        b"<!doctype html><main>Sentinal</main>",
    )
    .unwrap();
    fs::write(web.join("manifest.webmanifest"), b"{}").unwrap();
    fs::write(
        web.join("service-worker.js"),
        b"importScripts('/push-safety.js')",
    )
    .unwrap();
    fs::write(web.join("push-safety.js"), b"self.SentinalPushSafety = {}").unwrap();
    fs::write(web.join("assets/app-a1b2c3.js"), b"console.log('app')").unwrap();
    fs::write(web.join("secret.txt"), b"must not be exposed").unwrap();
    let oversized = fs::File::create(web.join("assets/oversized.js")).unwrap();
    oversized.set_len(8 * 1024 * 1024 + 1).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(web.join("secret.txt"), web.join("assets/linked.js")).unwrap();

    let app = package_router(
        PackageService::open(
            PackageServiceConfig {
                listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7422),
                directory: packages,
                public_base_url: "https://sentinal-control.example.ts.net".into(),
                max_artifact_bytes: 1024 * 1024,
                web_root: Some(web),
            },
            owner_uid(&root.0),
        )
        .unwrap(),
    );

    let route = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/app/sessions/linux%3A1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(route.status(), StatusCode::OK);
    assert_eq!(route.headers()["cache-control"], "no-cache");
    assert_eq!(route.headers()["x-content-type-options"], "nosniff");
    assert!(route.headers().contains_key("content-security-policy"));

    let asset = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/assets/app-a1b2c3.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(asset.status(), StatusCode::OK);
    assert_eq!(
        asset.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    assert_eq!(
        asset.headers()["content-type"],
        "text/javascript; charset=utf-8"
    );

    let push_safety = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/push-safety.js")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(push_safety.status(), StatusCode::OK);
    assert_eq!(push_safety.headers()["cache-control"], "no-cache");
    assert_eq!(
        push_safety.headers()["content-type"],
        "text/javascript; charset=utf-8"
    );

    for path in [
        "/api/v1/auth/status",
        "/secret.txt",
        "/assets/%2e%2e%2fsecret.txt",
        "/assets/oversized.js",
        "/assets/linked.js",
    ] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
}

#[tokio::test]
async fn package_requests_have_a_global_per_minute_bound() {
    let root = TestDir::new("rate");
    publish_fixture(&root.0, b"verified archive");
    let app = package_router(service(&root.0));
    for _ in 0..120 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/packages/v1/channels/stable.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let limited = app
        .oneshot(
            Request::builder()
                .uri("/packages/v1/channels/stable.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
#[cfg(unix)]
async fn package_root_symlink_is_refused() {
    use std::os::unix::fs::symlink;

    let root = TestDir::new("symlink");
    let real = root.0.join("real");
    let linked = root.0.join("linked");
    fs::create_dir(&real).unwrap();
    symlink(&real, &linked).unwrap();
    let error = PackageService::open(
        PackageServiceConfig {
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7422),
            directory: linked,
            public_base_url: "https://sentinal-control.example.ts.net".into(),
            max_artifact_bytes: 1024,
            web_root: None,
        },
        owner_uid(&real),
    )
    .unwrap_err();
    assert!(error.to_string().contains("symlink"));
}

#[tokio::test]
#[cfg(unix)]
async fn package_child_symlink_is_refused() {
    use std::os::unix::fs::symlink;

    let root = TestDir::new("child-symlink");
    let outside = root.0.join("outside");
    let packages = root.0.join("packages");
    fs::create_dir(&outside).unwrap();
    fs::create_dir(&packages).unwrap();
    symlink(&outside, packages.join("artifacts")).unwrap();
    let error = PackageService::open(
        PackageServiceConfig {
            listen: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7422),
            directory: packages,
            public_base_url: "https://sentinal-control.example.ts.net".into(),
            max_artifact_bytes: 1024,
            web_root: None,
        },
        owner_uid(&outside),
    )
    .unwrap_err();
    assert!(error.to_string().contains("symlink"));
}

#[cfg(unix)]
fn owner_uid(path: &Path) -> u32 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).unwrap().uid()
}

#[cfg(not(unix))]
fn owner_uid(_path: &Path) -> u32 {
    0
}

#[test]
#[cfg(unix)]
fn generated_bootstrap_verifies_archive_before_running_installer() {
    let root = TestDir::new("bootstrap");
    let release = root.0.join("release/sentinal-worker");
    fs::create_dir_all(release.join("bin")).unwrap();
    fs::create_dir_all(release.join("scripts")).unwrap();
    write_executable(&release.join("bin/sentinal"), "#!/bin/sh\nexit 0\n");
    write_executable(&release.join("bin/sentinald"), "#!/bin/sh\nexit 0\n");
    write_executable(
        &release.join("scripts/install-worker.sh"),
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >\"$SENTINAL_TEST_INSTALL_LOG\"\n",
    );
    let archive = root.0.join("worker.tar.gz");
    let tar_status = Command::new("tar")
        .args(["-czf"])
        .arg(&archive)
        .arg("-C")
        .arg(root.0.join("release"))
        .arg("sentinal-worker")
        .status()
        .unwrap();
    assert!(tar_status.success());
    let digest = sha256(&archive);
    let bytes = fs::metadata(&archive).unwrap().len();
    let (target, os, arch) = host_target();
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
            artifact_path: format!("/packages/v1/artifacts/sentinal-worker-0.0.1-{target}.tar.gz"),
            bytes,
            sha256: digest,
        }],
    };
    manifest.validate(1024 * 1024).unwrap();
    let script = root.0.join("install.sh");
    write_executable(
        &script,
        &render_install_script("https://sentinal-control.example.ts.net", &manifest),
    );
    let fake_bin = root.0.join("fake-bin");
    fs::create_dir_all(&fake_bin).unwrap();
    write_executable(
        &fake_bin.join("curl"),
        r#"#!/bin/sh
set -eu
[ "${SENTINAL_TEST_CURL_FAIL:-}" != 1 ] || exit 22
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '--output' ]; then output="$2"; shift 2; else shift; fi
done
cp "$SENTINAL_TEST_ARCHIVE" "$output"
"#,
    );
    write_executable(
        &fake_bin.join("tailscale"),
        "#!/bin/sh\n[ \"$1 $2\" = 'ip -4' ] || exit 2\nprintf '%s\\n' '100.90.80.70'\n",
    );
    write_executable(
        &fake_bin.join("sh"),
        "#!/bin/sh\nprintf '%s\\n' 'bootstrap invoked installer through PATH sh' >&2\nexit 91\n",
    );
    let install_log = root.0.join("installer.log");
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new("/bin/sh")
        .arg(&script)
        .env("PATH", &path)
        .env("SENTINAL_TEST_ARCHIVE", &archive)
        .env("SENTINAL_TEST_INSTALL_LOG", &install_log)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("State: UNPAIRED"));
    let invocation = fs::read_to_string(&install_log).unwrap();
    assert!(invocation.contains("install --release-dir"));
    assert!(invocation.contains("--bind 100.90.80.70"));
    assert!(!invocation.contains("--prefix"));

    let custom_prefix = root.0.join("custom prefix");
    let custom_install_log = root.0.join("custom-installer.log");
    let custom = Command::new("/bin/sh")
        .arg(&script)
        .arg("--prefix")
        .arg(&custom_prefix)
        .env("PATH", &path)
        .env("SENTINAL_TEST_ARCHIVE", &archive)
        .env("SENTINAL_TEST_INSTALL_LOG", &custom_install_log)
        .output()
        .unwrap();
    assert!(
        custom.status.success(),
        "{}",
        String::from_utf8_lossy(&custom.stderr)
    );
    let custom_invocation = fs::read_to_string(&custom_install_log).unwrap();
    assert!(custom_invocation.contains(&format!("--prefix {}", custom_prefix.display())));

    let fake_app_cli = fake_bin.join("TailscaleApp");
    fs::rename(fake_bin.join("tailscale"), &fake_app_cli).unwrap();
    let app_install_log = root.0.join("app-cli-installer.log");
    let app_cli = Command::new("/bin/sh")
        .arg(&script)
        .env("PATH", &path)
        .env("SENTINAL_TAILSCALE_CLI", &fake_app_cli)
        .env("SENTINAL_TEST_ARCHIVE", &archive)
        .env("SENTINAL_TEST_INSTALL_LOG", &app_install_log)
        .output()
        .unwrap();
    assert!(
        app_cli.status.success(),
        "{}",
        String::from_utf8_lossy(&app_cli.stderr)
    );
    assert!(app_install_log.exists());
    fs::rename(&fake_app_cli, fake_bin.join("tailscale")).unwrap();
    let rendered = fs::read_to_string(&script).unwrap();
    assert!(rendered.contains("/Applications/Tailscale.app/Contents/MacOS/Tailscale"));

    fs::write(&archive, b"corrupt").unwrap();
    let rejected = Command::new("/bin/sh")
        .arg(&script)
        .env("PATH", &path)
        .env("SENTINAL_TEST_ARCHIVE", &archive)
        .env("SENTINAL_TEST_INSTALL_LOG", root.0.join("must-not-run.log"))
        .output()
        .unwrap();
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("artifact length mismatch"));
    assert!(!root.0.join("must-not-run.log").exists());
}

#[cfg(unix)]
fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn sha256(path: &Path) -> String {
    let output = Command::new("sh")
        .arg("-c")
        .arg("if command -v sha256sum >/dev/null 2>&1; then sha256sum \"$1\"; else shasum -a 256 \"$1\"; fi")
        .arg("sentinal-sha256")
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
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
