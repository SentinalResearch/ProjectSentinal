//! Real zsh/bash/fish running the generated hooks against a fake local socket.
//!
//! Run with: `cargo test -p cli --test hook_script_integration -- --ignored --nocapture`.
//! Shells that are not installed are skipped and reported.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use protocol::LocalRequest;
use std::{path::Path, time::Duration};
use support::{find_shell, wait_until, FakeSocket, Mode, ShellTerm};

const OUTPUT_MARKER: &str = "TERMINAL-OUTPUT-MUST-NOT-BE-REPORTED";

fn sentinal() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_sentinal"))
}

fn status_line(shell: &str) -> &'static str {
    if shell == "fish" {
        "echo \"status=$status\""
    } else {
        "echo \"status=$?\""
    }
}

fn check_reporting(shell: &str) {
    let fake = FakeSocket::start(Mode::Answer);
    let marker_file = support::short_dir("mk").join("out.txt");
    std::fs::write(&marker_file, OUTPUT_MARKER).unwrap();
    let mut term = ShellTerm::spawn(
        shell,
        sentinal(),
        &[("SENTINAL_SOCKET", fake.path.display().to_string())],
    );
    let pid = term.pid();

    let metachar = "printf '%s\\n' 'a;b $(x) \"q\" | & >'";
    term.run("true");
    term.run("false");
    let (status, _) = term.run(status_line(shell));
    assert!(status.contains("status=1"), "{shell} lost $?: {status}");
    term.run("cd /tmp");
    term.run(metachar);
    let (output, _) = term.run(&format!("cat {}", marker_file.display()));
    assert!(output.contains(OUTPUT_MARKER));

    let finishes = |requests: &[LocalRequest]| {
        requests
            .iter()
            .filter(|request| matches!(request, LocalRequest::ShellCommandFinished { .. }))
            .count()
    };
    wait_until(&format!("{shell} to report 6 finishes"), || {
        finishes(&fake.requests()) >= 6
    });
    let requests = fake.requests();

    let mut starts: Vec<(u64, String, String)> = requests
        .iter()
        .filter_map(|request| match request {
            LocalRequest::ShellCommandStarted {
                sequence,
                text,
                cwd,
                pid: from,
                ..
            } => {
                assert_eq!(*from, pid);
                Some((*sequence, text.clone(), cwd.clone()))
            }
            _ => None,
        })
        .collect();
    starts.sort();
    let texts: Vec<&str> = starts.iter().map(|(_, text, _)| text.as_str()).collect();
    assert_eq!(
        texts,
        vec![
            "true",
            "false",
            status_line(shell),
            "cd /tmp",
            metachar,
            &format!("cat {}", marker_file.display()),
        ],
        "{shell} texts"
    );
    assert_eq!(starts[4].2, "/tmp", "{shell}: cwd after cd");

    let mut exits: Vec<(u64, i32)> = requests
        .iter()
        .filter_map(|request| match request {
            LocalRequest::ShellCommandFinished {
                sequence,
                exit_code,
                ..
            } => Some((*sequence, *exit_code)),
            _ => None,
        })
        .collect();
    exits.sort();
    assert_eq!(
        exits.iter().map(|(_, code)| *code).collect::<Vec<_>>(),
        vec![0, 1, 0, 0, 0, 0],
        "{shell} exit codes"
    );
    assert!(requests.iter().any(
        |request| matches!(request, LocalRequest::RegisterShell { pid: from, .. } if *from == pid)
    ));

    // Terminal output never reaches the socket.
    let raw = String::from_utf8_lossy(&fake.raw.lock().unwrap()).to_string();
    assert!(!raw.contains(OUTPUT_MARKER), "{shell} reported output");
    term.exit();
    eprintln!("{shell}: reporting ok ({} requests)", requests.len());
}

fn check_suppressed(shell: &str) {
    for (key, value) in [("SENTINAL_SESSION_ID", "42"), ("SENTINAL_DISABLE", "1")] {
        let fake = FakeSocket::start(Mode::Answer);
        let mut term = ShellTerm::spawn(
            shell,
            sentinal(),
            &[
                ("SENTINAL_SOCKET", fake.path.display().to_string()),
                (key, value.into()),
            ],
        );
        term.run("true");
        term.run("false");
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(
            fake.connections.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "{shell} reported with {key}={value}"
        );
        term.exit();
    }
    eprintln!("{shell}: suppression ok");
}

fn check_silent_failures(shell: &str) {
    let missing = support::short_dir("mis").join("absent.sock");
    let silent = FakeSocket::start(Mode::Silent);
    let garbage = FakeSocket::start(Mode::Garbage);
    for (label, socket) in [
        ("daemon down", missing),
        ("timeout", silent.path.clone()),
        ("malformed reply", garbage.path.clone()),
    ] {
        let mut term = ShellTerm::spawn(
            shell,
            sentinal(),
            &[("SENTINAL_SOCKET", socket.display().to_string())],
        );
        term.run("false");
        let (output, _) = term.run(status_line(shell));
        assert!(output.contains("status=1"), "{shell} {label}: {output}");
        std::thread::sleep(Duration::from_millis(200));
        let (quiet, _) = term.run("true");
        for noise in ["sentinal", "Error", "error", "[1]", "Done", "refused"] {
            assert!(
                !quiet.contains(noise) && !output.contains(noise),
                "{shell} {label} printed {noise:?}: {output}{quiet}"
            );
        }
        term.exit();
    }
    eprintln!("{shell}: silent failures ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "runs real zsh, bash, and fish in pseudo-terminals"]
async fn hook_script_integration() {
    let mut skipped = Vec::new();
    for shell in ["zsh", "bash", "fish"] {
        if find_shell(shell).is_none() {
            skipped.push(shell);
            continue;
        }
        tokio::task::block_in_place(|| {
            check_reporting(shell);
            check_suppressed(shell);
            check_silent_failures(shell);
        });
    }
    eprintln!("skipped (not installed): {skipped:?}");
}
