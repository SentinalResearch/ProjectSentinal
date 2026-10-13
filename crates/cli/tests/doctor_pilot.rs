//! `sentinal doctor --pilot` (4.E §9): read-only preflight of a control VM,
//! with Tailscale, the listener table, and HTTPS faked on PATH.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

const ORIGIN: &str = "https://pilot.example-tailnet.ts.net";

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Pilot {
    root: std::path::PathBuf,
}

impl Drop for Pilot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

impl Pilot {
    fn new(name: &str, serve: &str, listeners: &str, curl_ok: bool) -> Self {
        let root =
            std::env::temp_dir().join(format!("sentinal-pilot-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let (prefix, config, bin, home) = (
            root.join("prefix"),
            root.join("config"),
            root.join("bin"),
            root.join("home"),
        );
        for dir in [
            prefix.join("bin"),
            prefix.join("share/sentinal"),
            config.clone(),
            bin.clone(),
            home.join("sentinal-backups"),
        ] {
            fs::create_dir_all(dir).unwrap();
        }
        executable(&prefix.join("bin/sentinal"), "#!/bin/sh\nexit 0\n");
        executable(&prefix.join("bin/sentinald"), "#!/bin/sh\nexit 0\n");
        let packages = root.join("data/packages");
        fs::create_dir_all(packages.join("channels")).unwrap();
        fs::write(
            packages.join("channels/stable.json"),
            "{\"release_version\": \"0.1.19\", \"version\": \"0.1.19\"}",
        )
        .unwrap();
        fs::write(home.join("sentinal-backups/sentinal-backup-1.tar.gz"), b"x").unwrap();
        fs::write(
            config.join("daemon.toml"),
            format!(
                "bind = \"127.0.0.1:7420\"\ndata_dir = \"{data}\"\nbehind_tailscale_serve = true\nallowed_origins = [\"{ORIGIN}\"]\n\n[control]\nenabled = true\npublic_url = \"{ORIGIN}\"\n\n[packages]\nenabled = true\nlisten = \"127.0.0.1:7422\"\ndirectory = \"{packages}\"\npublic_base_url = \"{ORIGIN}\"\n",
                data = root.join("data").display(),
                packages = packages.display()
            ),
        )
        .unwrap();
        executable(
            &bin.join("tailscale"),
            &format!("#!/bin/sh\ncase \"$1 $2\" in\n  'serve status') echo '{serve}' ;;\n  'ip -4') echo 100.90.80.70 ;;\n  *) echo connected ;;\nesac\n"),
        );
        executable(&bin.join("lsof"), &format!("#!/bin/sh\nprintf '%s\\n' 'COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME' {listeners}\n"));
        let curl = if curl_ok {
            "#!/bin/sh\ncase \"$*\" in *health*) echo '{\"ok\":true,\"version\":1}' ;; *) echo ok ;; esac\n"
        } else {
            "#!/bin/sh\nexit 22\n"
        };
        executable(&bin.join("curl"), curl);
        for service in ["launchctl", "systemctl"] {
            executable(&bin.join(service), "#!/bin/sh\nexit 0\n");
        }
        Self { root }
    }

    fn run(&self) -> (bool, String) {
        let output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
            .args(["doctor", "--pilot", "--prefix"])
            .arg(self.root.join("prefix"))
            .arg("--config-dir")
            .arg(self.root.join("config"))
            .env("HOME", self.root.join("home"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.join("bin").display()),
            )
            .output()
            .unwrap();
        (
            output.status.success(),
            String::from_utf8(output.stdout).unwrap(),
        )
    }
}

const GOOD_SERVE: &str = r#"{"TCP":{"443":{"HTTPS":true}},"Web":{"pilot.example-tailnet.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:7422"},"/api":{"Proxy":"http://127.0.0.1:7420/api"}}}}}"#;
const LOOPBACK: &str = "'sentinald 1 u 9u IPv4 0 0t0 TCP 127.0.0.1:7420 (LISTEN)' 'sentinald 1 u 10u IPv4 0 0t0 TCP 127.0.0.1:7422 (LISTEN)' 'sshd 2 root 3u IPv4 0 0t0 TCP *:22 (LISTEN)'";

#[test]
fn a_correct_pilot_control_passes_every_check() {
    let pilot = Pilot::new("good", GOOD_SERVE, LOOPBACK, true);
    let (ok, stdout) = pilot.run();
    for line in [
        "Pilot listeners: OK (7420 and 7422 on loopback only)",
        "Pilot Serve: OK (/ and /api on 443; Funnel off)",
        "Pilot HTTPS: OK (health, dashboard, install.sh)",
        "Pilot packages: OK (stable 0.1.19)",
        "Pilot backup: OK (newest is less than a day old)",
    ] {
        assert!(stdout.contains(line), "{line}\n{stdout}");
    }
    assert!(ok, "{stdout}");
}

#[test]
fn exposure_funnel_and_dead_https_are_errors() {
    let funnel = r#"{"AllowFunnel":{"pilot.example-tailnet.ts.net:443":true}}"#;
    let exposed = "'sentinald 1 u 9u IPv4 0 0t0 TCP *:7420 (LISTEN)'";
    let pilot = Pilot::new("bad", funnel, exposed, false);
    let (ok, stdout) = pilot.run();
    assert!(!ok);
    assert!(
        stdout.contains("Pilot listeners: ERROR (*:7420 listens beyond loopback)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("Pilot Serve: ERROR") && stdout.contains("Funnel"),
        "{stdout}"
    );
    assert!(stdout.contains("Pilot HTTPS: ERROR"), "{stdout}");
}
