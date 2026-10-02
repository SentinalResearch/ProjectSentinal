//! Manual real-Tailnet enrollment through Tailscale Serve.
//!
//! On a running worker, with a code created in the control's Settings →
//! Machines → Add Machine and written to an owner-only file:
//!
//! ```console
//! $ umask 077; printf '%s\n' 7KQ4M-2XZ9C > /tmp/sentinal-code
//! $ SENTINAL_ENROLL_CONTROL_URL=https://sentinal-control.<tailnet>.ts.net \
//!   SENTINAL_ENROLL_CODE_FILE=/tmp/sentinal-code \
//!   cargo test -p cli --test enrollment_tailnet_integration -- --ignored
//! ```
//!
//! The worker must not already be connected. The test connects, checks the
//! link is ACTIVE, then disconnects again so the owner sees DISCONNECTED.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{os::unix::fs::PermissionsExt, process::Command};

fn sentinal(args: &[&str], stdin: Option<&[u8]>) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_sentinal"));
    command.args(args);
    match stdin {
        Some(input) => {
            use std::io::Write;
            let mut child = command
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            child.wait_with_output().unwrap()
        }
        None => command.output().unwrap(),
    }
}

#[test]
#[ignore = "needs a real control behind Tailscale Serve and an owner-issued code"]
fn enrollment_tailnet_integration() {
    let url = std::env::var("SENTINAL_ENROLL_CONTROL_URL")
        .expect("set SENTINAL_ENROLL_CONTROL_URL to the control's https .ts.net origin");
    assert!(
        url.starts_with("https://") && url.ends_with(".ts.net"),
        "{url}"
    );
    let code_file = std::env::var("SENTINAL_ENROLL_CODE_FILE")
        .expect("set SENTINAL_ENROLL_CODE_FILE to an owner-only file holding the code");
    let mode = std::fs::metadata(&code_file).unwrap().permissions().mode();
    assert_eq!(
        mode & 0o077,
        0,
        "the code file must be readable only by you"
    );
    let code = std::fs::read(&code_file).unwrap();

    let connect = sentinal(&["connect", &url, "--code-stdin"], Some(&code));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&connect.stdout),
        String::from_utf8_lossy(&connect.stderr)
    );
    let code_text = String::from_utf8_lossy(&code);
    assert!(!text.contains(code_text.trim()), "the code was printed");
    assert_eq!(connect.status.code(), Some(0), "{text}");

    let status = sentinal(&["connection", "status", "--json"], None);
    let report: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(report["state"], "ACTIVE");
    assert_eq!(report["control_url"], url.as_str());

    let disconnect = sentinal(&["disconnect"], None);
    assert_eq!(disconnect.status.code(), Some(0));
    let _ = std::fs::remove_file(code_file);
}
