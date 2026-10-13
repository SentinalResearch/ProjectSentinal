//! `sentinal setup` (4.E §3): the CONTROL preview/apply and role choice, with
//! Tailscale, the service manager, and publishing replaced by a recording fake.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::setup::{run, Outcome, ServeRoute, SetupOptions, SetupRole, System, TailscaleSelf};
use std::{
    fs,
    io::Cursor,
    net::Ipv4Addr,
    path::{Path, PathBuf},
};

const DNS: &str = "pilot.example-tailnet.ts.net";
const ORIGIN: &str = "https://pilot.example-tailnet.ts.net";

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sentinal-setup-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct Fake {
    tailscale_missing: bool,
    serve: serde_json::Value,
    linger: Option<bool>,
    restart_fails: bool,
    unhealthy: bool,
    discovered: cli::discovery::Discovered,
    password_needed: Option<bool>,
    typed: std::collections::VecDeque<String>,
    password_set: Option<String>,
    calls: Vec<String>,
}

impl System for Fake {
    fn tailscale(&mut self) -> Result<TailscaleSelf, String> {
        if self.tailscale_missing {
            return Err("Tailscale is not installed".into());
        }
        Ok(TailscaleSelf {
            dns_name: DNS.into(),
            ipv4: Ipv4Addr::new(100, 90, 80, 70),
        })
    }
    fn serve_status(&mut self) -> Result<serde_json::Value, String> {
        Ok(self.serve.clone())
    }
    fn serve(&mut self, route: ServeRoute) -> Result<(), String> {
        self.calls.push(format!("serve {route:?}"));
        Ok(())
    }
    fn linger(&mut self, _user: &str) -> Option<bool> {
        self.linger
    }
    fn enable_linger(&mut self, user: &str) -> Result<(), String> {
        self.calls.push(format!("linger {user}"));
        self.linger = Some(true);
        Ok(())
    }
    fn restart_service(&mut self) -> Result<(), String> {
        self.calls.push("restart".into());
        if self.restart_fails {
            Err("unit failed".into())
        } else {
            Ok(())
        }
    }
    fn healthy(&mut self, url: &str) -> bool {
        self.calls.push(format!("health {url}"));
        !self.unhealthy
    }
    fn publish(&mut self, version: &str, _config_dir: &Path) -> Result<(), String> {
        self.calls.push(format!("publish {version}"));
        Ok(())
    }
    fn discover(&mut self) -> Result<cli::discovery::Discovered, String> {
        Ok(self.discovered.clone())
    }
    fn password_needed(&mut self) -> Option<bool> {
        self.password_needed
    }
    fn read_password(&mut self, _prompt: &str) -> Option<String> {
        self.typed.pop_front()
    }
    fn set_password(&mut self, token_file: &Path, password: &str) -> Result<(), String> {
        self.calls.push(format!(
            "set password with {}",
            token_file.file_name().unwrap().to_string_lossy()
        ));
        self.password_set = Some(password.to_string());
        self.password_needed = Some(false);
        Ok(())
    }
}

struct Setup {
    dir: TestDir,
    config_dir: PathBuf,
    data_dir: PathBuf,
}

/// What `install.sh` leaves behind: a worker config bound to the Tailscale address.
const INSTALLER_CONFIG: &str = "# written by install.sh\nbind = \"100.90.80.70:7420\"\ndata_dir = \"DATA\"\nscrollback_lines = 10000\n";

impl Setup {
    fn new(name: &str) -> Self {
        let dir = TestDir::new(name);
        let config_dir = dir.0.join("config");
        let data_dir = dir.0.join("data");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(dir.0.join("prefix/share/sentinal/web")).unwrap();
        fs::write(
            config_dir.join("daemon.toml"),
            INSTALLER_CONFIG.replace("DATA", &data_dir.display().to_string()),
        )
        .unwrap();
        Self {
            dir,
            config_dir,
            data_dir,
        }
    }

    fn options(&self, role: Option<SetupRole>, yes: bool) -> SetupOptions {
        SetupOptions {
            role,
            name: None,
            control_url: None,
            yes,
            interactive: true,
            prefix: self.dir.0.join("prefix"),
            config_dir: self.config_dir.clone(),
            home: self.dir.0.clone(),
            user: "owner".into(),
            version: "0.1.18".into(),
        }
    }

    fn run(
        &self,
        options: SetupOptions,
        fake: &mut Fake,
        input: &str,
    ) -> (Outcome, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let outcome = run(
            options,
            fake,
            &mut Cursor::new(input.as_bytes().to_vec()),
            &mut out,
            &mut err,
        );
        (
            outcome,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    fn config(&self) -> String {
        fs::read_to_string(self.config_dir.join("daemon.toml")).unwrap()
    }
}

fn linux() -> Fake {
    Fake {
        serve: serde_json::json!({}),
        linger: Some(false),
        ..Fake::default()
    }
}

#[test]
fn control_setup_previews_then_applies_a_config_the_daemon_accepts() {
    let setup = Setup::new("control");
    let mut fake = linux();
    let (outcome, stdout, stderr) = setup.run(setup.options(None, false), &mut fake, "1\n\ny\n");
    assert_eq!(outcome, Outcome::Exit(0), "{stdout}{stderr}");

    let preview = stdout.find("Preview").expect("a preview is shown");
    let apply = stdout.find("Apply?").unwrap();
    assert!(preview < apply);
    for expected in [
        "127.0.0.1:7420",
        "127.0.0.1:7422",
        "never 0.0.0.0",
        "sudo loginctl enable-linger owner",
        ORIGIN,
        "Funnel: never",
        "interrupts live FULL Sessions",
    ] {
        assert!(
            stdout[preview..apply].contains(expected),
            "{expected}: {stdout}"
        );
    }
    assert!(stdout.contains(&format!("Open {ORIGIN}")), "{stdout}");

    let config = setup.config();
    assert!(
        config.starts_with("# written by install.sh\n"),
        "comments kept: {config}"
    );
    let parsed = daemon::config::DaemonConfig::from_toml(&config, &setup.dir.0).unwrap();
    assert!(parsed.bind.ip().is_loopback(), "{config}");
    assert_eq!(parsed.bind.port(), 7420);
    let control = parsed.control.expect("[control] enabled");
    assert_eq!(control.public_url.trim_end_matches('/'), ORIGIN);
    let packages = parsed.packages.expect("[packages] enabled");
    assert!(packages.listen.ip().is_loopback());
    assert_eq!(packages.directory, setup.data_dir.join("packages"));
    assert_eq!(packages.public_base_url, ORIGIN);
    assert_eq!(
        packages.web_root,
        Some(setup.dir.0.join("prefix/share/sentinal/web"))
    );
    assert!(parsed.behind_tailscale_serve);
    assert_eq!(parsed.allowed_origins, vec![ORIGIN.to_string()]);
    assert_eq!(
        fs::read_to_string(setup.config_dir.join("daemon.toml.before-setup")).unwrap(),
        INSTALLER_CONFIG.replace("DATA", &setup.data_dir.display().to_string())
    );

    assert_eq!(
        fake.calls,
        vec![
            "linger owner".to_string(),
            "restart".into(),
            "health http://127.0.0.1:7420/api/v1/health".into(),
            "serve Root".into(),
            "serve Api".into(),
            format!("health {ORIGIN}/api/v1/health"),
            "publish 0.1.18".into(),
        ]
    );
}

#[test]
fn rerunning_on_a_finished_control_changes_nothing() {
    let setup = Setup::new("rerun");
    let mut fake = linux();
    let (outcome, ..) = setup.run(setup.options(Some(SetupRole::Control), true), &mut fake, "");
    assert_eq!(outcome, Outcome::Exit(0));
    let config = setup.config();
    fs::create_dir_all(setup.data_dir.join("packages/releases")).unwrap();
    fs::write(setup.data_dir.join("packages/releases/0.1.18.json"), "{}").unwrap();

    let mut again = Fake {
        serve: serde_json::json!({"Web": {format!("{DNS}:443"): {"Handlers": {
            "/": {"Proxy": "http://127.0.0.1:7422"},
            "/api": {"Proxy": "http://127.0.0.1:7420/api"}}}}}),
        linger: Some(true),
        ..Fake::default()
    };
    let (outcome, stdout, _) = setup.run(setup.options(None, false), &mut again, "1\n");
    assert_eq!(outcome, Outcome::Exit(0));
    assert!(stdout.contains("Already a Sentinal control"), "{stdout}");
    assert!(stdout.contains("Nothing to do"), "{stdout}");
    assert!(!stdout.contains("Apply?"), "{stdout}");
    assert_eq!(setup.config(), config);
    assert!(
        again.calls.iter().all(|call| call.starts_with("health")),
        "{:?}",
        again.calls
    );
}

#[test]
fn declining_or_quitting_changes_nothing() {
    for input in ["1\n\nn\n", "q\n", "1\n\n\n", ""] {
        let setup = Setup::new("cancel");
        let before = setup.config();
        let mut fake = linux();
        let (outcome, _, stderr) = setup.run(setup.options(None, false), &mut fake, input);
        assert_eq!(outcome, Outcome::Exit(2), "{input:?}");
        assert!(
            stderr.contains("nothing was changed"),
            "{input:?}: {stderr}"
        );
        assert_eq!(setup.config(), before);
        assert!(!setup.config_dir.join("daemon.toml.before-setup").exists());
        assert!(fake.calls.is_empty(), "{input:?}: {:?}", fake.calls);
    }
}

#[test]
fn a_foreign_serve_route_or_funnel_is_never_overwritten() {
    let foreign = serde_json::json!({"Web": {format!("{DNS}:443"): {"Handlers": {"/": {"Proxy": "http://127.0.0.1:3000"}}}}});
    let funnel = serde_json::json!({"AllowFunnel": {format!("{DNS}:443"): true}});
    for (serve, message) in [(foreign, "127.0.0.1:3000"), (funnel, "Funnel")] {
        let setup = Setup::new("serve-conflict");
        let before = setup.config();
        let mut fake = Fake {
            serve,
            linger: Some(true),
            ..Fake::default()
        };
        let (outcome, _, stderr) =
            setup.run(setup.options(Some(SetupRole::Control), true), &mut fake, "");
        assert_eq!(outcome, Outcome::Exit(5), "{stderr}");
        assert!(stderr.contains(message), "{stderr}");
        assert_eq!(setup.config(), before);
        assert!(fake.calls.is_empty(), "{:?}", fake.calls);
    }
}

#[test]
fn a_connected_worker_is_not_turned_into_a_control() {
    let setup = Setup::new("conflict");
    fs::write(setup.config_dir.join("link.json"), "{}").unwrap();
    let before = setup.config();
    let mut fake = linux();
    let (outcome, _, stderr) =
        setup.run(setup.options(Some(SetupRole::Control), true), &mut fake, "");
    assert_eq!(outcome, Outcome::Exit(5));
    assert!(stderr.contains("sentinal disconnect"), "{stderr}");
    assert_eq!(setup.config(), before);
}

#[test]
fn a_control_is_not_offered_the_worker_role() {
    let setup = Setup::new("control-to-worker");
    let mut fake = linux();
    setup.run(setup.options(Some(SetupRole::Control), true), &mut fake, "");
    let mut fake = linux();
    let (outcome, _, stderr) =
        setup.run(setup.options(Some(SetupRole::Worker), true), &mut fake, "");
    assert_eq!(outcome, Outcome::Exit(5));
    assert!(stderr.contains("daemon.toml"), "{stderr}");
}

#[test]
fn missing_tailscale_is_a_prerequisite_error() {
    let setup = Setup::new("no-tailscale");
    let mut fake = Fake {
        tailscale_missing: true,
        ..Fake::default()
    };
    let (outcome, _, stderr) =
        setup.run(setup.options(Some(SetupRole::Control), true), &mut fake, "");
    assert_eq!(outcome, Outcome::Exit(6));
    assert!(stderr.contains("Tailscale"), "{stderr}");
}

#[test]
fn a_failed_restart_restores_the_previous_config() {
    for (restart_fails, unhealthy) in [(true, false), (false, true)] {
        let setup = Setup::new("rollback");
        let before = setup.config();
        let mut fake = Fake {
            restart_fails,
            unhealthy,
            ..linux()
        };
        let (outcome, _, stderr) =
            setup.run(setup.options(Some(SetupRole::Control), true), &mut fake, "");
        assert_eq!(outcome, Outcome::Exit(1), "{stderr}");
        assert!(stderr.contains("restored"), "{stderr}");
        assert_eq!(setup.config(), before);
        assert!(!fake.calls.iter().any(|call| call.starts_with("serve")));
    }
}

#[test]
fn without_a_terminal_setup_needs_a_role_and_yes() {
    let setup = Setup::new("no-tty");
    let mut fake = linux();
    let mut options = setup.options(None, false);
    options.interactive = false;
    let (outcome, _, stderr) = setup.run(options, &mut fake, "");
    assert_eq!(outcome, Outcome::Exit(2));
    assert!(stderr.contains("--role"), "{stderr}");
    assert!(fake.calls.is_empty());
}

#[test]
fn the_worker_role_hands_a_validated_control_url_to_connect() {
    let setup = Setup::new("worker");
    let mut fake = linux();
    let (outcome, stdout, _) = setup.run(
        setup.options(None, false),
        &mut fake,
        &format!("3\n{ORIGIN}\n"),
    );
    assert_eq!(
        outcome,
        Outcome::Connect {
            control_url: ORIGIN.into(),
            name: None
        },
        "{stdout}"
    );
    assert!(fake.calls.is_empty());

    let mut fake = linux();
    let (outcome, _, stderr) = setup.run(
        setup.options(None, false),
        &mut fake,
        "3\nhttp://192.168.1.5\n",
    );
    assert_eq!(outcome, Outcome::Exit(2));
    assert!(stderr.contains("https"), "{stderr}");
}

#[test]
fn top_level_keys_go_above_existing_tables_and_those_tables_survive() {
    use cli::setup::config::{control_config, ConfigChange, ControlInputs};
    let existing = "bind = \"100.90.80.70:7420\"\n\n[storage]\nsession_log_max_age_days = 30\n";
    let ConfigChange::Write(text) = control_config(
        Some(existing),
        &ControlInputs {
            origin: ORIGIN,
            display_name: "Lab \"Sentinal\"",
            package_dir: Path::new("/srv/packages"),
            web_root: None,
        },
    )
    .unwrap() else {
        panic!("expected a write");
    };
    let parsed = daemon::config::DaemonConfig::from_toml(&text, Path::new("/home/owner")).unwrap();
    assert!(parsed.behind_tailscale_serve, "{text}");
    assert_eq!(parsed.storage.session_log_max_age_days, 30, "{text}");
    let value: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(
        value["control"]["display_name"].as_str(),
        Some("Lab \"Sentinal\"")
    );
    assert!(control_config(
        Some("bind = \"0.0.0.0:7420\"\n"),
        &ControlInputs {
            origin: ORIGIN,
            display_name: "x",
            package_dir: Path::new("/srv/packages"),
            web_root: None,
        }
    )
    .is_err());
}

fn found(hosts: &[(&str, &str)]) -> cli::discovery::Discovered {
    cli::discovery::Discovered {
        controls: hosts
            .iter()
            .map(|(host, name)| cli::discovery::FoundControl {
                discovery: protocol::pairing::DiscoveryResponse {
                    sentinal: "control".into(),
                    schema_version: 1,
                    control_machine_id: protocol::MachineId("01K2C7RQ9M3X8V6T4N2P0SDB5A".into()),
                    display_name: (*name).into(),
                    origin: format!("https://{host}"),
                    release: "0.1.18".into(),
                    protocol: protocol::pairing::ProtocolRange { min: 1, max: 1 },
                    pairing: protocol::pairing::PairingSupport {
                        approval: false,
                        code: true,
                    },
                },
                address: "100.101.1.1".parse().unwrap(),
            })
            .collect(),
        older: vec!["https://old.example-tailnet.ts.net".into()],
        checked: hosts.len() + 2,
        unanswered: 1,
    }
}

#[test]
fn discovered_controls_are_listed_and_the_owner_picks_one() {
    let setup = Setup::new("discover");
    let mut fake = Fake {
        discovered: found(&[
            ("lab.example-tailnet.ts.net", "Lab"),
            (DNS, "Owner's Sentinal"),
        ]),
        ..linux()
    };
    let (outcome, stdout, _) = setup.run(setup.options(None, false), &mut fake, "2\n2\n");
    assert_eq!(
        outcome,
        Outcome::Connect {
            control_url: ORIGIN.into(),
            name: None
        },
        "{stdout}"
    );
    for expected in [
        "1) Lab   https://lab.example-tailnet.ts.net   0.1.18 · control 01K2C7RQ",
        "2) Owner's Sentinal",
        "3) Enter a URL",
        "older Sentinal at https://old.example-tailnet.ts.net",
        "(4 peers checked; 1 did not answer)",
    ] {
        assert!(stdout.contains(expected), "{expected}: {stdout}");
    }
}

#[test]
fn a_single_control_is_never_picked_without_asking() {
    let setup = Setup::new("discover-one");
    let mut fake = Fake {
        discovered: found(&[(DNS, "Owner's Sentinal")]),
        ..linux()
    };
    let (outcome, stdout, _) = setup.run(setup.options(None, false), &mut fake, "2\n");
    assert_eq!(outcome, Outcome::Exit(2), "{stdout}");
    assert!(stdout.contains("Choose 1-2"), "{stdout}");
}

#[test]
fn with_no_control_found_the_owner_types_a_url() {
    let setup = Setup::new("discover-none");
    let mut fake = linux();
    let (outcome, stdout, _) = setup.run(
        setup.options(None, false),
        &mut fake,
        &format!("2\n{ORIGIN}\n"),
    );
    assert!(stdout.contains("No Sentinal control found"), "{stdout}");
    assert_eq!(
        outcome,
        Outcome::Connect {
            control_url: ORIGIN.into(),
            name: None
        }
    );
}

#[test]
fn a_control_that_supports_approval_is_asked_to_approve() {
    let setup = Setup::new("discover-approve");
    let mut discovered = found(&[(DNS, "Owner's Sentinal")]);
    discovered.controls[0].discovery.pairing.approval = true;
    let mut fake = Fake {
        discovered,
        ..linux()
    };
    let (outcome, stdout, _) = setup.run(setup.options(None, false), &mut fake, "2\n1\n");
    assert_eq!(
        outcome,
        Outcome::Approve {
            control_url: ORIGIN.into(),
            control_machine_id: protocol::MachineId("01K2C7RQ9M3X8V6T4N2P0SDB5A".into()),
            control_name: "Owner's Sentinal".into(),
            name: None,
        },
        "{stdout}"
    );
}

const PASSWORD: &str = "correct horse battery";

#[test]
fn the_control_sets_the_owner_password_without_showing_the_token() {
    let setup = Setup::new("password");
    let mut fake = Fake {
        password_needed: Some(true),
        typed: ["short", PASSWORD, "different password", PASSWORD, PASSWORD]
            .map(String::from)
            .into(),
        ..linux()
    };
    let (outcome, stdout, stderr) =
        setup.run(setup.options(Some(SetupRole::Control), true), &mut fake, "");
    assert_eq!(outcome, Outcome::Exit(0), "{stderr}");
    assert_eq!(fake.password_set.as_deref(), Some(PASSWORD));
    assert!(
        fake.calls.contains(&"set password with token".to_string()),
        "{:?}",
        fake.calls
    );
    assert!(stdout.contains("at least 12 characters"), "{stdout}");
    assert!(stdout.contains("did not match"), "{stdout}");
    assert!(stdout.contains("✓ owner password"), "{stdout}");
    assert!(
        stdout.contains(&format!("Open {ORIGIN} and sign in")),
        "{stdout}"
    );
    assert!(!stdout.contains(PASSWORD) && !stderr.contains(PASSWORD));
}

#[test]
fn rerunning_asks_for_a_missing_password_and_cancelling_skips_it() {
    let setup = Setup::new("password-rerun");
    setup.run(
        setup.options(Some(SetupRole::Control), true),
        &mut linux(),
        "",
    );
    fs::create_dir_all(setup.data_dir.join("packages/releases")).unwrap();
    fs::write(setup.data_dir.join("packages/releases/0.1.18.json"), "{}").unwrap();
    let serve = serde_json::json!({"Web": {format!("{DNS}:443"): {"Handlers": {
        "/": {"Proxy": "http://127.0.0.1:7422"},
        "/api": {"Proxy": "http://127.0.0.1:7420/api"}}}}});

    let mut fake = Fake {
        serve: serve.clone(),
        linger: Some(true),
        password_needed: Some(true),
        typed: [PASSWORD, PASSWORD].map(String::from).into(),
        ..Fake::default()
    };
    let (outcome, stdout, _) = setup.run(setup.options(None, false), &mut fake, "1\n");
    assert_eq!(outcome, Outcome::Exit(0));
    assert_eq!(fake.password_set.as_deref(), Some(PASSWORD), "{stdout}");

    let mut cancelled = Fake {
        serve,
        linger: Some(true),
        password_needed: Some(true),
        ..Fake::default()
    };
    let (outcome, stdout, _) = setup.run(setup.options(None, false), &mut cancelled, "1\n");
    assert_eq!(outcome, Outcome::Exit(0));
    assert!(cancelled.password_set.is_none());
    assert!(stdout.contains("run sentinal setup again"), "{stdout}");
}

#[test]
fn without_a_terminal_the_password_is_left_for_later() {
    let setup = Setup::new("password-no-tty");
    let mut fake = Fake {
        password_needed: Some(true),
        typed: [PASSWORD, PASSWORD].map(String::from).into(),
        ..linux()
    };
    let mut options = setup.options(Some(SetupRole::Control), true);
    options.interactive = false;
    let (outcome, stdout, _) = setup.run(options, &mut fake, "");
    assert_eq!(outcome, Outcome::Exit(0));
    assert!(
        fake.password_set.is_none(),
        "never prompts without a terminal"
    );
    assert!(
        stdout.contains("run sentinal setup in a terminal"),
        "{stdout}"
    );
}
