//! `sentinal backup create|restore` (4.E §9): the service is stopped around
//! the copy, the archive is owner-only and outside the backed-up directories,
//! and restore refuses anything but a stopped, empty install.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::backup::{create, restore, BackupPaths, ServiceControl};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("sentinal-backup-{name}-{}", std::process::id()));
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
    running: bool,
    calls: Vec<&'static str>,
}

impl ServiceControl for Fake {
    fn stop(&mut self) -> Result<(), String> {
        self.calls.push("stop");
        self.running = false;
        Ok(())
    }
    fn start(&mut self) -> Result<(), String> {
        self.calls.push("start");
        self.running = true;
        Ok(())
    }
    fn is_running(&mut self) -> bool {
        self.running
    }
}

fn install(home: &Path) -> BackupPaths {
    let config = home.join(".config/terminal-agent");
    let data = home.join(".local/share/terminal-agent");
    fs::create_dir_all(data.join("sessions")).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(config.join("daemon.toml"), "bind = \"127.0.0.1:7420\"\n").unwrap();
    fs::write(
        config.join("machine-id.json"),
        "{\"machine_id\":\"01J9Z3K8V6Q2T7M4X5C0RNB8DA\"}",
    )
    .unwrap();
    fs::write(data.join("terminal-agent.sqlite3"), b"sqlite").unwrap();
    fs::write(data.join("sessions/1.log.zst"), b"log").unwrap();
    BackupPaths {
        home: home.to_path_buf(),
        config_dir: config,
        data_dir: data,
    }
}

#[test]
fn create_stops_the_service_and_writes_an_owner_only_archive_of_both_directories() {
    let dir = TestDir::new("create");
    let paths = install(&dir.0.join("home"));
    let output = dir.0.join("out/pilot.tar.gz");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    let mut service = Fake {
        running: true,
        ..Fake::default()
    };
    let mut out = Vec::new();
    create(&paths, &output, &mut service, &mut out).unwrap();

    assert_eq!(
        service.calls,
        vec!["stop", "start"],
        "the service is restarted after the copy"
    );
    assert_eq!(
        fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let listing = String::from_utf8(
        Command::new("tar")
            .arg("-tzf")
            .arg(&output)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    for entry in [
        ".config/terminal-agent/daemon.toml",
        ".config/terminal-agent/machine-id.json",
        ".local/share/terminal-agent/terminal-agent.sqlite3",
        ".local/share/terminal-agent/sessions/1.log.zst",
    ] {
        assert!(
            listing
                .lines()
                .any(|line| line.trim_end_matches('/') == entry),
            "{entry}: {listing}"
        );
    }
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.contains("credentials"),
        "warns that the archive holds secrets: {text}"
    );
}

#[test]
fn create_refuses_an_output_inside_the_backup_or_an_existing_file_and_still_restarts() {
    let dir = TestDir::new("refuse");
    let paths = install(&dir.0.join("home"));
    let existing = dir.0.join("existing.tar.gz");
    fs::write(&existing, b"keep me").unwrap();
    for output in [
        paths.data_dir.join("backup.tar.gz"),
        paths.config_dir.join("b.tar.gz"),
        existing.clone(),
    ] {
        let mut service = Fake {
            running: true,
            ..Fake::default()
        };
        assert!(
            create(&paths, &output, &mut service, &mut Vec::new()).is_err(),
            "{}",
            output.display()
        );
        assert!(
            service.calls.is_empty(),
            "nothing is stopped for a refused output"
        );
    }
    assert_eq!(fs::read(&existing).unwrap(), b"keep me");
}

#[test]
fn restore_needs_a_stopped_empty_install_and_puts_both_directories_back() {
    let dir = TestDir::new("restore");
    let source = install(&dir.0.join("old home"));
    let archive = dir.0.join("pilot.tar.gz");
    create(
        &source,
        &archive,
        &mut Fake {
            running: true,
            ..Fake::default()
        },
        &mut Vec::new(),
    )
    .unwrap();

    let home = dir.0.join("new home");
    let target = BackupPaths {
        home: home.clone(),
        config_dir: home.join(".config/terminal-agent"),
        data_dir: home.join(".local/share/terminal-agent"),
    };
    let mut running = Fake {
        running: true,
        ..Fake::default()
    };
    assert!(restore(&target, &archive, &mut running, &mut Vec::new())
        .unwrap_err()
        .contains("stop"));

    fs::create_dir_all(&target.data_dir).unwrap();
    fs::write(target.data_dir.join("terminal-agent.sqlite3"), b"new").unwrap();
    assert!(
        restore(&target, &archive, &mut Fake::default(), &mut Vec::new())
            .unwrap_err()
            .contains("not empty")
    );
    fs::remove_dir_all(&target.data_dir).unwrap();

    restore(&target, &archive, &mut Fake::default(), &mut Vec::new()).unwrap();
    assert_eq!(
        fs::read(target.data_dir.join("sessions/1.log.zst")).unwrap(),
        b"log"
    );
    assert!(
        fs::read_to_string(target.config_dir.join("machine-id.json"))
            .unwrap()
            .contains("01J9Z3K8")
    );
    assert_eq!(
        fs::metadata(&target.config_dir)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}

#[test]
fn restore_refuses_archives_with_unexpected_or_escaping_entries() {
    let dir = TestDir::new("hostile");
    let staging = dir.0.join("staging");
    fs::create_dir_all(staging.join(".ssh")).unwrap();
    fs::write(staging.join(".ssh/authorized_keys"), b"evil").unwrap();
    let archive = dir.0.join("hostile.tar.gz");
    assert!(Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .arg(".ssh")
        .status()
        .unwrap()
        .success());
    let home = dir.0.join("home");
    let target = BackupPaths {
        home: home.clone(),
        config_dir: home.join(".config/terminal-agent"),
        data_dir: home.join(".local/share/terminal-agent"),
    };
    let error = restore(&target, &archive, &mut Fake::default(), &mut Vec::new()).unwrap_err();
    assert!(error.contains(".ssh"), "{error}");
    assert!(!home.join(".ssh").exists());
}
