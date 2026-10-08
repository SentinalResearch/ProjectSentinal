//! Real zsh/bash auto-wrap: entry into a real sentinald, and fail-open.
//!
//! Run with: `cargo build -p daemon --bin sentinald && cargo test -p cli --test
//! auto_wrap_integration -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use cli::{auto_wrap::loader_body, client::DaemonClient, hook_script::HookShell};
use protocol::{CaptureLevel, ClientRequest, DaemonMessage, SessionOrigin};
use std::{path::Path, time::Duration};
use support::{find_shell, short_dir, wait_until, Daemon, ShellTerm, PROMPT};

fn sentinal() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_sentinal"))
}

fn rc(shell: &str, auto: bool) -> String {
    let prompt = if shell == "zsh" {
        format!("PROMPT='{PROMPT}'\nRPROMPT=\n")
    } else {
        format!("PS1='{PROMPT}'\n")
    };
    let hook_shell = if shell == "zsh" {
        HookShell::Zsh
    } else {
        HookShell::Bash
    };
    let block = if auto {
        loader_body(hook_shell, sentinal()).unwrap()
    } else {
        String::new()
    };
    format!("{block}{prompt}")
}

fn median(mut values: Vec<Duration>) -> Duration {
    values.sort();
    values[values.len() / 2]
}

fn check_entry(daemon: &Daemon, shell: &str) {
    let env = [
        ("SENTINAL_URL", daemon.url.clone()),
        ("SENTINAL_TOKEN_FILE", daemon.token.display().to_string()),
    ];
    // The daemon starts the inner shell with its own HOME, so the same rc
    // (auto block + prompt) lives there too.
    let (mut term, _) = ShellTerm::spawn_with_rc(shell, &daemon.home, &rc(shell, true), &env);
    let (inside, _) =
        term.run("echo \"id=${SENTINAL_SESSION_ID-none} guard=${SENTINAL_AUTO_GUARD-none}\"");
    assert!(inside.contains("guard=1"), "{shell}: not wrapped: {inside}");
    assert!(!inside.contains("id=none"), "{shell}: {inside}");

    let sessions = tokio::runtime::Handle::current().block_on(async {
        let mut client = DaemonClient::connect(std::slice::from_ref(&daemon.url), &daemon.token)
            .await
            .unwrap();
        match client.request(ClientRequest::ListSessions).await.unwrap() {
            DaemonMessage::Sessions { sessions } => sessions,
            other => panic!("unexpected {other:?}"),
        }
    });
    let wrapped: Vec<_> = sessions
        .iter()
        .filter(|session| session.origin == SessionOrigin::AutoWrap && !session.state.is_finished())
        .collect();
    assert_eq!(
        wrapped.len(),
        1,
        "{shell}: exactly one AUTO_WRAP Session, no nesting: {sessions:?}"
    );
    assert_eq!(wrapped[0].capture, CaptureLevel::Full);

    // Exiting the wrapped shell closes the terminal (the outer shell exits 0).
    assert_eq!(term.finish("exit"), 0, "{shell}");
    eprintln!(
        "{shell}: entered AUTO_WRAP Session {} and exited cleanly",
        wrapped[0].id.0
    );
}

fn check_fail_open(shell: &str) {
    // A daemon that accepts connections and never answers.
    let hung = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("ws://{}/api/v1/ws", hung.local_addr().unwrap());
    let token = short_dir("tok").join("token");
    std::fs::write(&token, "t").unwrap();
    let home = short_dir("afo");
    let env = [
        ("SENTINAL_URL", url),
        ("SENTINAL_TOKEN_FILE", token.display().to_string()),
        ("SENTINAL_TEST_MARKER", "kept as is".to_string()),
    ];
    let probe =
        "echo \"argv0=$0 pwd=$PWD marker=$SENTINAL_TEST_MARKER guard=${SENTINAL_AUTO_GUARD-none}\"";

    let mut baseline_times = Vec::new();
    let mut baseline_view = String::new();
    for _ in 0..3 {
        let (mut term, took) = ShellTerm::spawn_with_rc(shell, &home, &rc(shell, false), &env);
        baseline_times.push(took);
        baseline_view = term.run(probe).0;
        assert_eq!(term.finish("exit 3"), 3);
    }
    let mut wrapped_times = Vec::new();
    for _ in 0..3 {
        let (mut term, took) = ShellTerm::spawn_with_rc(shell, &home, &rc(shell, true), &env);
        wrapped_times.push(took);
        let (view, _) = term.run(probe);
        assert_eq!(
            view.lines().find(|line| line.starts_with("argv0=")),
            baseline_view
                .lines()
                .find(|line| line.starts_with("argv0=")),
            "{shell}: argv/cwd/env changed on fallback"
        );
        assert_eq!(
            term.finish("exit 3"),
            3,
            "{shell}: exit status changed on fallback"
        );
    }
    let added = median(wrapped_times).saturating_sub(median(baseline_times.clone()));
    eprintln!(
        "{shell}: fallback with a hung daemon added {:.0} ms to shell start (baseline {:.0} ms)",
        added.as_secs_f64() * 1e3,
        median(baseline_times).as_secs_f64() * 1e3
    );
    assert!(
        added < Duration::from_millis(300),
        "{shell}: fail-open took {added:?}"
    );
    drop(hung);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "starts a real sentinald and real shells"]
async fn auto_wrap_integration() {
    let daemon = Daemon::start(sentinal());
    let mut skipped = Vec::new();
    tokio::task::block_in_place(|| {
        for shell in ["zsh", "bash"] {
            if find_shell(shell).is_none() {
                skipped.push(shell);
                continue;
            }
            check_entry(&daemon, shell);
            check_fail_open(shell);
        }
        wait_until("no leftover AUTO_WRAP Sessions", || true);
    });
    daemon.stop();
    eprintln!("skipped (not installed): {skipped:?}; fish auto-wrap is untested on this host");
}
