//! Manual benchmark: prompt time added by the external hook, per shell.
//!
//! Run with: `cargo test --release -p cli --test hook_benchmark -- --ignored --nocapture`.
//! Each iteration types `true` into a real interactive shell in a PTY and
//! measures the time until the next prompt appears. The baseline loads the
//! same rc file with `SENTINAL_DISABLE=1` (the hook defines nothing); the
//! hooked run reports every Command to a fake local socket. Added time at a
//! percentile is `hooked - baseline` at that percentile. Gate: p95 < 5 ms.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::{path::Path, time::Duration};
use support::{find_shell, shell_version, FakeSocket, Mode, ShellTerm};

const WARMUP: usize = 50;
const ITERATIONS: usize = 500;
const P95_GATE: Duration = Duration::from_millis(5);

fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    let index = ((sorted.len() as f64 - 1.0) * fraction).round() as usize;
    sorted[index]
}

fn measure(shell: &str, env: &[(&str, String)]) -> Vec<Duration> {
    let mut term = ShellTerm::spawn(shell, Path::new(env!("CARGO_BIN_EXE_sentinal")), env);
    for _ in 0..WARMUP {
        term.run("true");
    }
    let mut samples: Vec<Duration> = (0..ITERATIONS).map(|_| term.run("true").1).collect();
    term.exit();
    samples.sort();
    samples
}

fn ms(duration: Duration) -> String {
    format!("{:.2}", duration.as_secs_f64() * 1_000.0)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "manual timing benchmark with real shells"]
async fn hook_benchmark() {
    let os = std::process::Command::new("uname")
        .arg("-srm")
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    println!("OS: {os}; warmup {WARMUP}; iterations {ITERATIONS}");
    println!("| shell | version | baseline p50/p95/max ms | hooked p50/p95/max ms | added p50 | added p95 | added max |");
    println!("|---|---|---|---|---:|---:|---:|");
    let mut failures = Vec::new();
    for shell in ["zsh", "bash", "fish"] {
        let Some(path) = find_shell(shell) else {
            println!("| {shell} | not installed | | | | | |");
            continue;
        };
        let fake = FakeSocket::start(Mode::Answer);
        let socket = ("SENTINAL_SOCKET", fake.path.display().to_string());
        let (baseline, hooked) = tokio::task::block_in_place(|| {
            let baseline = measure(shell, &[socket.clone(), ("SENTINAL_DISABLE", "1".into())]);
            let hooked = measure(shell, std::slice::from_ref(&socket));
            (baseline, hooked)
        });
        assert!(
            fake.connections.load(std::sync::atomic::Ordering::SeqCst) > ITERATIONS,
            "{shell}: the hooked run did not report"
        );
        let added = |fraction| {
            percentile(&hooked, fraction).saturating_sub(percentile(&baseline, fraction))
        };
        let (b, h) = (&baseline, &hooked);
        println!(
            "| {shell} | {} | {}/{}/{} | {}/{}/{} | {} | {} | {} |",
            shell_version(&path),
            ms(percentile(b, 0.5)),
            ms(percentile(b, 0.95)),
            ms(percentile(b, 1.0)),
            ms(percentile(h, 0.5)),
            ms(percentile(h, 0.95)),
            ms(percentile(h, 1.0)),
            ms(added(0.5)),
            ms(added(0.95)),
            ms(added(1.0)),
        );
        if added(0.95) >= P95_GATE {
            failures.push(format!("{shell} added p95 {} ms", ms(added(0.95))));
        }
    }
    assert!(
        failures.is_empty(),
        "p95 gate (< 5 ms) failed: {failures:?}"
    );
}
