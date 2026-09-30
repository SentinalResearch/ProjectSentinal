//! CLI parsing, detach sequence, and terminal framing tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use clap::Parser;
use cli::{
    args::{Cli, Command},
    detach::DetachDetector,
    frame::{decode_terminal_frame, encode_terminal_frame},
};
use protocol::SessionId;
use std::path::Path;

#[test]
fn parses_list_command() {
    let cli = Cli::try_parse_from(["ta", "ls"]).unwrap();

    assert!(matches!(cli.command, Command::List));
}

#[test]
fn parses_new_working_directory() {
    let cli = Cli::try_parse_from(["ta", "new", "--cwd", "/tmp/project"]).unwrap();

    assert!(matches!(
        cli.command,
        Command::New { cwd: Some(path) } if path == Path::new("/tmp/project")
    ));
}

#[test]
fn parses_attach_and_kill_session_ids() {
    let attach = Cli::try_parse_from(["ta", "attach", "42"]).unwrap();
    let kill = Cli::try_parse_from(["ta", "kill", "99"]).unwrap();

    assert!(matches!(attach.command, Command::Attach { id: 42 }));
    assert!(matches!(kill.command, Command::Kill { id: 99 }));
}

#[test]
fn parses_doctor_with_optional_install_paths() {
    let cli = Cli::try_parse_from([
        "sentinal",
        "doctor",
        "--prefix",
        "/opt/sentinal",
        "--config-dir",
        "/private/sentinal-config",
    ])
    .unwrap();

    assert!(matches!(
        cli.command,
        Command::Doctor { prefix: Some(prefix), config_dir: Some(config_dir) }
            if prefix == Path::new("/opt/sentinal")
                && config_dir == Path::new("/private/sentinal-config")
    ));
}

#[test]
fn parses_update_check_and_explicit_rollback() {
    let check = Cli::try_parse_from(["sentinal", "update", "--check"]).unwrap();
    assert!(matches!(
        check.command,
        Command::Update {
            check: true,
            rollback: None,
            prefix: None
        }
    ));

    let rollback = Cli::try_parse_from([
        "sentinal",
        "update",
        "--rollback",
        "0.0.1",
        "--prefix",
        "/opt/sentinal",
    ])
    .unwrap();
    assert!(matches!(
        rollback.command,
        Command::Update { check: false, rollback: Some(version), prefix: Some(prefix) }
            if version == "0.0.1" && prefix == Path::new("/opt/sentinal")
    ));
    assert!(Cli::try_parse_from(["sentinal", "update", "--check", "--rollback", "0.0.1"]).is_err());
}

#[test]
fn rejects_attach_without_session_id() {
    assert!(Cli::try_parse_from(["ta", "attach"]).is_err());
}

#[test]
fn detach_sequence_is_removed_from_terminal_input() {
    let mut detector = DetachDetector::new();

    let first = detector.push(b"hello\x1c");
    let second = detector.push(b"dignored");

    assert_eq!(first.bytes, b"hello");
    assert!(!first.detach);
    assert!(second.bytes.is_empty());
    assert!(second.detach);
}

#[test]
fn control_backslash_not_followed_by_d_is_forwarded() {
    let mut detector = DetachDetector::new();

    let first = detector.push(b"\x1c");
    let second = detector.push(b"x");

    assert!(first.bytes.is_empty());
    assert_eq!(second.bytes, b"\x1cx");
    assert!(!second.detach);
}

#[test]
fn cli_terminal_frame_round_trips() {
    let encoded = encode_terminal_frame(SessionId(42), b"output");

    let (session_id, payload) = decode_terminal_frame(&encoded).unwrap();

    assert_eq!(session_id, SessionId(42));
    assert_eq!(payload, b"output");
}

#[test]
fn global_url_and_token_file_can_come_from_the_environment() {
    let parsed = Cli::try_parse_from(["sentinal", "ls"]).unwrap();
    assert_eq!(parsed.url, None);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .arg("ls")
        .env("SENTINAL_URL", "ws://127.0.0.1:9/api/v1/ws")
        .env("SENTINAL_TOKEN_FILE", "/nonexistent/sentinal-token")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("/nonexistent/sentinal-token"), "{stderr}");
}
