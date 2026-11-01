//! `sentinal complete` (Milestone 5.A) against a fake local socket.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use protocol::LocalRequest;
use std::{path::Path, process::Command};
use support::{FakeSocket, Mode};

fn sentinal(socket: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sentinal"));
    command
        .env("SENTINAL_SOCKET", socket)
        .env_remove("SENTINAL_COMPLETE_PREFIX")
        .env_remove("SENTINAL_SESSION_ID");
    command
}

#[tokio::test(flavor = "multi_thread")]
async fn prints_one_completion_per_line_and_reads_the_prefix_from_the_environment() {
    let fake = FakeSocket::start(Mode::Answer);
    let output = tokio::task::spawn_blocking({
        let path = fake.path.clone();
        move || {
            sentinal(&path)
                .args(["complete", "--cwd", "/work", "--limit", "4"])
                // The typed text travels in the environment, never argv (`ps`).
                .env("SENTINAL_COMPLETE_PREFIX", "echo s")
                .output()
                .unwrap()
        }
    })
    .await
    .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "echo sentinal-completed-first | tr a-z A-Z\necho sentinal-completed-second | tr a-z A-Z\n"
    );
    let requests = fake.requests();
    assert!(
        matches!(
            &requests[..],
            [LocalRequest::Complete { prefix, cwd: Some(cwd), limit: 4, include_failed: false, session_id: None }]
                if prefix == "echo s" && cwd == "/work"
        ),
        "{requests:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn json_output_and_an_empty_answer() {
    let fake = FakeSocket::start(Mode::Answer);
    let path = fake.path.clone();
    let (json, empty) = tokio::task::spawn_blocking(move || {
        let json = sentinal(&path)
            .args(["complete", "--json", "--include-failed", "--", "ech"])
            .output()
            .unwrap();
        let empty = sentinal(&path).args(["complete"]).output().unwrap();
        (json, empty)
    })
    .await
    .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(
        parsed[0]["text"],
        "echo sentinal-completed-first | tr a-z A-Z"
    );
    assert_eq!(parsed[0]["count"], 3);
    assert!(empty.status.success());
    assert!(empty.stdout.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_silent_daemon_fails_fast_and_quietly() {
    let fake = FakeSocket::start(Mode::Silent);
    let path = fake.path.clone();
    let started = std::time::Instant::now();
    let output = tokio::task::spawn_blocking(move || {
        sentinal(&path)
            .args(["complete", "--", "x"])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    // A key press must never hang the shell.
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
}
