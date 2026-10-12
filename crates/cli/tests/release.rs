//! `sentinal release`: the control's signed-release check, update with health
//! check and automatic rollback, explicit rollback, and update policy. Fake
//! curl (GitHub API, downloads, health), fake release tools, real ssh-keygen.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
const TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

struct Fixture {
    root: PathBuf,
    prefix: PathBuf,
    config: PathBuf,
    data: PathBuf,
    packages: PathBuf,
    downloads: PathBuf,
    fake_bin: PathBuf,
    tool_log: PathBuf,
    allowed: PathBuf,
    key: PathBuf,
    stranger: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn keygen(path: &Path) {
    assert!(Command::new("ssh-keygen")
        .args(["-q", "-t", "ed25519", "-N", "", "-C", "test", "-f"])
        .arg(path)
        .status()
        .unwrap()
        .success());
}

impl Fixture {
    fn new(role_control: bool) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "sentinal-release-{}-{nonce}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let prefix = root.join("prefix");
        let config = root.join("config");
        let data = root.join("data");
        let packages = data.join("packages");
        let downloads = root.join("github");
        let fake_bin = root.join("fake-bin");
        for dir in [&config, &data, &packages, &downloads, &fake_bin] {
            fs::create_dir_all(dir).unwrap();
        }
        fs::create_dir_all(prefix.join("share/sentinal")).unwrap();
        fs::write(
            prefix.join("share/sentinal/release.json"),
            r#"{"schema_version":1,"version":"0.1.17","target":"x86_64-unknown-linux-musl","channel":"stable","control_url":""}"#,
        )
        .unwrap();
        let control = if role_control {
            "[control]\nenabled = true\npublic_url = \"https://control.example.ts.net\"\n"
        } else {
            ""
        };
        fs::write(
            config.join("daemon.toml"),
            format!(
                "bind = \"127.0.0.1:7420\"\ndata_dir = \"{}\"\n{control}[packages]\nenabled = true\ndirectory = \"{}\"\npublic_base_url = \"https://control.example.ts.net\"\n",
                data.display(),
                packages.display()
            ),
        )
        .unwrap();

        // Fake release tools that record their arguments.
        let tool_log = root.join("tools.log");
        for tool in [
            "receive-release.sh",
            "publish-worker-release.sh",
            "apply-release.sh",
        ] {
            write_executable(
                &prefix.join("share/sentinal/tools/scripts").join(tool),
                &format!(
                    "#!/bin/sh\nprintf '{tool} %s\\n' \"$*\" >>'{}'\n[ \"${{SENTINAL_TEST_TOOL_FAIL:-}}\" != '{tool}' ] || exit 7\necho '{tool} ran'\n",
                    tool_log.display()
                ),
            );
        }

        let key = root.join("release-key");
        let stranger = root.join("stranger-key");
        keygen(&key);
        keygen(&stranger);
        let public = fs::read_to_string(key.with_extension("pub")).unwrap();
        let mut fields = public.split_whitespace();
        let allowed = root.join("allowed_signers");
        fs::write(
            &allowed,
            format!(
                "sentinal-release namespaces=\"sentinal-release\" {} {}\n",
                fields.next().unwrap(),
                fields.next().unwrap()
            ),
        )
        .unwrap();

        // curl: the GitHub API list, release downloads by tag, and daemon health.
        write_executable(
            &fake_bin.join("curl"),
            &format!(
                r#"#!/bin/sh
output=''
url=''
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output|-o) output="$2"; shift 2 ;;
    http://*|https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
printf '%s\n' "$url" >>'{root}/curl.log'
emit() {{ if [ -n "$output" ]; then cp "$1" "$output"; else cat "$1"; fi; }}
case "$url" in
  https://api.github.com/repos/example/sentinal/releases*) emit '{root}/api.json' ;;
  https://github.com/example/sentinal/releases/download/*)
    file='{downloads}'/"$(printf '%s' "$url" | sed 's#.*/download/##; s#/#__#')"
    [ -f "$file" ] || exit 22
    emit "$file" ;;
  http://127.0.0.1:7420/api/v1/health)
    [ "${{SENTINAL_TEST_HEALTH:-ok}}" = ok ] || exit 7
    printf '{{"ok":true,"version":1}}' ;;
  *) exit 22 ;;
esac
"#,
                root = root.display(),
                downloads = downloads.display()
            ),
        );
        Self {
            root,
            prefix,
            config,
            data,
            packages,
            downloads,
            fake_bin,
            tool_log,
            allowed,
            key,
            stranger,
        }
    }

    /// Publish a GitHub release `tag` with signed SHA256SUMS (or unsigned).
    fn github_release(&self, version: &str, signer: Option<&Path>) {
        let sums = self.downloads.join(format!("v{version}__SHA256SUMS"));
        let mut text = String::new();
        for target in TARGETS {
            text.push_str(&format!(
                "{}  sentinal-worker-{version}-{target}.tar.gz\n",
                "a".repeat(64)
            ));
        }
        fs::write(&sums, text).unwrap();
        if let Some(key) = signer {
            assert!(Command::new("ssh-keygen")
                .args(["-q", "-Y", "sign", "-n", "sentinal-release", "-f"])
                .arg(key)
                .arg(&sums)
                .status()
                .unwrap()
                .success());
            // ssh-keygen writes v<version>__SHA256SUMS.sig, the download name.
            assert!(self
                .downloads
                .join(format!("v{version}__SHA256SUMS.sig"))
                .exists());
        }
    }

    fn api(&self, releases: &[(&str, bool, bool)]) {
        let entries: Vec<String> = releases
            .iter()
            .map(|(version, draft, prerelease)| {
                format!(
                    r#"{{"tag_name":"v{version}","draft":{draft},"prerelease":{prerelease},"published_at":"2026-10-12T10:00:00Z","html_url":"https://github.com/example/sentinal/releases/tag/v{version}","assets":[{{"name":"SHA256SUMS"}},{{"name":"SHA256SUMS.sig"}}]}}"#
                )
            })
            .collect();
        fs::write(
            self.root.join("api.json"),
            format!("[{}]", entries.join(",")),
        )
        .unwrap();
    }

    fn run(&self, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_sentinal"));
        command
            .arg("release")
            .args(args)
            .args(["--repo", "example/sentinal", "--allowed-signers"])
            .arg(&self.allowed)
            .arg("--prefix")
            .arg(&self.prefix)
            .arg("--config-dir")
            .arg(&self.config)
            .env("PATH", format!("{}:/usr/bin:/bin", self.fake_bin.display()))
            .stdin(Stdio::null());
        for (key, value) in env {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    fn tools(&self) -> String {
        fs::read_to_string(&self.tool_log).unwrap_or_default()
    }
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn check_offers_only_the_newest_owner_signed_release_on_the_channel() {
    let f = Fixture::new(true);
    f.github_release("0.1.18", Some(&f.key));
    f.github_release("0.1.19", Some(&f.key));
    f.github_release("0.1.20", Some(&f.key));
    f.github_release("0.1.16", Some(&f.key));
    // 0.1.20 is a draft, 0.1.19 a pre-release (beta), 0.1.16 older than installed.
    f.api(&[
        ("0.1.20", true, false),
        ("0.1.19", false, true),
        ("0.1.18", false, false),
        ("0.1.16", false, false),
    ]);

    let output = f.run(&["check"], &[]);
    let out = text(&output);
    assert!(output.status.success(), "{out}");
    assert!(
        out.contains("Control 0.1.17 (stable) · 0.1.18 available, signed by a trusted key"),
        "{out}"
    );
    assert!(out.contains("sentinal release update"), "{out}");
    let watch = fs::read_to_string(f.data.join("release-watch.json")).unwrap();
    assert!(
        watch.contains("\"version\":\"0.1.18\"") && watch.contains("\"signed\":true"),
        "{watch}"
    );
    assert_eq!(
        fs::metadata(f.data.join("release-watch.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let beta = f.run(&["check", "--channel", "beta", "--json"], &[]);
    let json: serde_json::Value = serde_json::from_slice(&beta.stdout).unwrap();
    assert_eq!(json["available"]["version"], "0.1.19", "{}", text(&beta));
    assert_eq!(json["channel"], "beta");
}

#[test]
fn check_reports_but_never_offers_an_unsigned_or_wrongly_signed_release() {
    let f = Fixture::new(true);
    f.github_release("0.1.18", Some(&f.stranger));
    f.api(&[("0.1.18", false, false)]);
    let output = f.run(&["check", "--json"], &[]);
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(json["available"].is_null(), "{}", text(&output));
    assert_eq!(json["unsigned_found"], "0.1.18");

    fs::remove_file(f.downloads.join("v0.1.18__SHA256SUMS.sig")).unwrap();
    let out = text(&f.run(&["check"], &[]));
    assert!(
        out.contains("0.1.18 found but not signed by a trusted key"),
        "{out}"
    );
    assert!(out.contains("0.1.17 is current"), "{out}");
}

#[test]
fn release_commands_refuse_on_a_worker() {
    let f = Fixture::new(false);
    for args in [
        vec!["check"],
        vec!["update", "--yes"],
        vec!["rollback", "0.1.16", "--yes"],
    ] {
        let output = f.run(&args, &[]);
        assert!(!output.status.success());
        assert!(
            text(&output).contains("this Machine is a worker"),
            "{}",
            text(&output)
        );
    }
    assert!(f.tools().is_empty());
}

#[test]
fn update_needs_confirmation_then_runs_the_verified_tools_and_checks_health() {
    let f = Fixture::new(true);
    f.github_release("0.1.18", Some(&f.key));
    f.api(&[("0.1.18", false, false)]);

    let refused = f.run(&["update"], &[]);
    assert!(!refused.status.success());
    assert!(text(&refused).contains("--yes"), "{}", text(&refused));
    assert!(f.tools().is_empty());

    let output = f.run(&["update", "--yes", "--health-timeout", "0"], &[]);
    let out = text(&output);
    assert!(output.status.success(), "{out}");
    let tools = f.tools();
    assert!(
        tools.contains("receive-release.sh --version 0.1.18 --package-dir"),
        "{tools}"
    );
    assert!(tools.contains(&f.packages.display().to_string()), "{tools}");
    assert!(tools.contains("--allowed-signers"), "{tools}");
    assert!(out.contains("Control updated to 0.1.18"), "{out}");
    let watch = fs::read_to_string(f.data.join("release-watch.json")).unwrap();
    assert!(watch.contains("\"state\":\"UPDATED\""), "{watch}");
}

#[test]
fn a_failed_health_check_rolls_back_to_the_previous_release() {
    let f = Fixture::new(true);
    f.github_release("0.1.18", Some(&f.key));
    f.api(&[("0.1.18", false, false)]);
    // The previous release is published locally, so it can be restored.
    fs::create_dir_all(f.packages.join("releases")).unwrap();
    fs::write(f.packages.join("releases/0.1.17.json"), "{}").unwrap();

    let output = f.run(
        &["update", "--yes", "--health-timeout", "0"],
        &[("SENTINAL_TEST_HEALTH", "fail")],
    );
    let out = text(&output);
    assert!(!output.status.success(), "{out}");
    assert!(out.contains("Rolled back to 0.1.17"), "{out}");
    let tools = f.tools();
    assert!(
        tools.contains("publish-worker-release.sh rollback --version 0.1.17"),
        "{tools}"
    );
    assert!(
        tools.contains("apply-release.sh --version 0.1.17"),
        "{tools}"
    );
    assert!(tools.contains("--rollback"), "{tools}");
    let watch = fs::read_to_string(f.data.join("release-watch.json")).unwrap();
    assert!(watch.contains("\"state\":\"ROLLED_BACK\""), "{watch}");
}

#[test]
fn update_reports_current_without_running_tools_and_refuses_downgrades() {
    let f = Fixture::new(true);
    f.api(&[]);
    let out = text(&f.run(&["update", "--yes"], &[]));
    assert!(out.contains("0.1.17 is current"), "{out}");
    let downgrade = f.run(&["update", "--yes", "--version", "0.1.16"], &[]);
    assert!(!downgrade.status.success());
    assert!(
        text(&downgrade).contains("sentinal release rollback"),
        "{}",
        text(&downgrade)
    );
    assert!(f.tools().is_empty());
}

#[test]
fn explicit_rollback_moves_the_channel_and_reinstalls_the_signed_release() {
    let f = Fixture::new(true);
    fs::create_dir_all(f.packages.join("releases")).unwrap();
    fs::write(f.packages.join("releases/0.1.16.json"), "{}").unwrap();
    let output = f.run(
        &["rollback", "0.1.16", "--yes", "--health-timeout", "0"],
        &[],
    );
    let out = text(&output);
    assert!(output.status.success(), "{out}");
    let tools = f.tools();
    assert!(
        tools.contains("publish-worker-release.sh rollback --version 0.1.16"),
        "{tools}"
    );
    assert!(
        tools.contains("apply-release.sh --version 0.1.16"),
        "{tools}"
    );
    assert!(out.contains("Control rolled back to 0.1.16"), "{out}");

    let missing = f.run(&["rollback", "0.1.10", "--yes"], &[]);
    assert!(!missing.status.success());
    assert!(
        text(&missing).contains("0.1.10 is not published"),
        "{}",
        text(&missing)
    );
}

#[test]
fn policy_is_stored_owner_only_and_validated() {
    let f = Fixture::new(true);
    let shown = text(&f.run(&["policy"], &[]));
    assert!(
        shown.contains("notify") && shown.contains("stable"),
        "{shown}"
    );
    assert!(f
        .run(&["policy", "auto", "--channel", "beta"], &[])
        .status
        .success());
    let stored = fs::read_to_string(f.data.join("release-policy.json")).unwrap();
    assert!(
        stored.contains("\"policy\":\"AUTOMATIC_WHEN_IDLE\"")
            && stored.contains("\"channel\":\"beta\""),
        "{stored}"
    );
    assert_eq!(
        fs::metadata(f.data.join("release-policy.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(text(&f.run(&["policy"], &[])).contains("automatic when idle"));
    assert!(!f.run(&["policy", "sometimes"], &[]).status.success());
}
