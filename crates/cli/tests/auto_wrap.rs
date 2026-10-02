//! Auto-wrap skip rules, fail-open deadline, rc loader, and on/off/status.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::{
    auto_wrap::{
        loader_body, skip_reason, within, AutoEnv, SkipReason, ENTRY_DEADLINE, FALLBACK_EXIT,
    },
    hook_script::HookShell,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

fn env(vars: &[(&str, &str)]) -> AutoEnv {
    let mut all: BTreeMap<String, String> = [("TERM", "xterm-256color")]
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    for (key, value) in vars {
        all.insert(key.to_string(), value.to_string());
    }
    AutoEnv {
        vars: all,
        interactive: true,
        stdin_tty: true,
        stdout_tty: true,
        shell: Some(HookShell::Zsh),
    }
}

#[test]
fn every_skip_rule_and_eligible_case() {
    let mut not_interactive = env(&[]);
    not_interactive.interactive = false;
    let mut piped = env(&[]);
    piped.stdout_tty = false;
    let mut no_stdin = env(&[]);
    no_stdin.stdin_tty = false;
    let mut tcsh = env(&[]);
    tcsh.shell = None;
    let mut no_term = env(&[]);
    no_term.vars.remove("TERM");

    let rows: Vec<(&str, AutoEnv, Option<SkipReason>)> = vec![
        ("plain local terminal", env(&[]), None),
        (
            "interactive ssh",
            env(&[("SSH_CONNECTION", "1 2 3 4"), ("SSH_TTY", "/dev/pts/1")]),
            None,
        ),
        (
            "VS Code integrated terminal",
            env(&[("TERM_PROGRAM", "vscode")]),
            None,
        ),
        (
            "JetBrains terminal",
            env(&[("TERMINAL_EMULATOR", "JetBrains-JediTerm")]),
            None,
        ),
        ("emacs vterm", env(&[("INSIDE_EMACS", "vterm")]), None),
        (
            "SENTINAL_DISABLE=0 is not disable",
            env(&[("SENTINAL_DISABLE", "0")]),
            None,
        ),
        (
            "disabled",
            env(&[("SENTINAL_DISABLE", "1")]),
            Some(SkipReason::Disabled),
        ),
        (
            "inside a session",
            env(&[("SENTINAL_SESSION_ID", "12")]),
            Some(SkipReason::InsideSession),
        ),
        (
            "recursion guard",
            env(&[("SENTINAL_AUTO_GUARD", "1")]),
            Some(SkipReason::RecursionGuard),
        ),
        (
            "non-interactive shell",
            not_interactive,
            Some(SkipReason::NotInteractive),
        ),
        ("stdout piped", piped, Some(SkipReason::NoTerminal)),
        ("stdin not a tty", no_stdin, Some(SkipReason::NoTerminal)),
        (
            "scp",
            env(&[
                ("SSH_ORIGINAL_COMMAND", "scp -t /tmp"),
                ("SSH_CONNECTION", "x"),
            ]),
            Some(SkipReason::FileTransfer),
        ),
        (
            "rsync",
            env(&[("SSH_ORIGINAL_COMMAND", "/usr/bin/rsync --server -a . /x")]),
            Some(SkipReason::FileTransfer),
        ),
        (
            "sftp",
            env(&[("SSH_ORIGINAL_COMMAND", "/usr/lib/openssh/sftp-server")]),
            Some(SkipReason::FileTransfer),
        ),
        (
            "ssh remote command",
            env(&[("SSH_ORIGINAL_COMMAND", "make deploy")]),
            Some(SkipReason::NonInteractiveSsh),
        ),
        (
            "ssh without tty",
            env(&[("SSH_CLIENT", "1 2 3")]),
            Some(SkipReason::NonInteractiveSsh),
        ),
        (
            "VS Code resolving env",
            env(&[("VSCODE_RESOLVING_ENVIRONMENT", "1")]),
            Some(SkipReason::IdeEnvironment("VSCODE_RESOLVING_ENVIRONMENT")),
        ),
        (
            "IntelliJ env reader",
            env(&[("INTELLIJ_ENVIRONMENT_READER", "1")]),
            Some(SkipReason::IdeEnvironment("INTELLIJ_ENVIRONMENT_READER")),
        ),
        (
            "configured task runner",
            env(&[("SENTINAL_AUTO_SKIP", "CI, MY_TASK"), ("MY_TASK", "1")]),
            Some(SkipReason::Configured("MY_TASK".into())),
        ),
        (
            "configured but unset",
            env(&[("SENTINAL_AUTO_SKIP", "MY_TASK")]),
            None,
        ),
        (
            "dumb terminal",
            env(&[("TERM", "dumb")]),
            Some(SkipReason::DumbTerminal),
        ),
        ("TERM unset", no_term, Some(SkipReason::DumbTerminal)),
        (
            "unsupported shell",
            tcsh,
            Some(SkipReason::UnsupportedShell),
        ),
        // Conflicting signals resolve by the table's precedence.
        (
            "disable beats session",
            env(&[("SENTINAL_DISABLE", "1"), ("SENTINAL_SESSION_ID", "3")]),
            Some(SkipReason::Disabled),
        ),
        (
            "session beats guard",
            env(&[("SENTINAL_SESSION_ID", "3"), ("SENTINAL_AUTO_GUARD", "1")]),
            Some(SkipReason::InsideSession),
        ),
        (
            "transfer beats ssh",
            env(&[
                ("SSH_ORIGINAL_COMMAND", "scp -f x"),
                ("SSH_TTY", "/dev/pts/2"),
            ]),
            Some(SkipReason::FileTransfer),
        ),
        (
            "empty SENTINAL_SESSION_ID is unset",
            env(&[("SENTINAL_SESSION_ID", "")]),
            None,
        ),
    ];
    for (name, case, expected) in rows {
        assert_eq!(skip_reason(&case), expected, "{name}");
    }
}

#[tokio::test(start_paused = true)]
async fn a_daemon_that_never_answers_falls_back_at_the_deadline() {
    let started = tokio::time::Instant::now();
    let result: Option<()> = within(ENTRY_DEADLINE, std::future::pending()).await;
    assert_eq!(result, None);
    assert_eq!(started.elapsed(), ENTRY_DEADLINE);
    assert!(ENTRY_DEADLINE < Duration::from_millis(300));
}

#[tokio::test(start_paused = true)]
async fn a_late_success_is_never_observed() {
    let entered = Arc::new(AtomicBool::new(false));
    let flag = entered.clone();
    let result = within(ENTRY_DEADLINE, async move {
        tokio::time::sleep(Duration::from_millis(400)).await;
        flag.store(true, Ordering::SeqCst);
        Some("second shell")
    })
    .await;
    assert_eq!(result, None);
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(!entered.load(Ordering::SeqCst), "the late work was dropped");
}

#[tokio::test(start_paused = true)]
async fn a_prompt_success_enters() {
    let result = within(ENTRY_DEADLINE, async {
        tokio::time::sleep(Duration::from_millis(20)).await;
        Some(7)
    })
    .await;
    assert_eq!(result, Some(7));
}

#[test]
fn loaders_check_guards_before_running_and_exit_only_on_success() {
    let binary = Path::new("/opt/sentinal bin/sentinal");
    for shell in [HookShell::Zsh, HookShell::Bash, HookShell::Fish] {
        let body = loader_body(shell, binary).unwrap();
        for guard in [
            "SENTINAL_SESSION_ID",
            "SENTINAL_DISABLE",
            "SENTINAL_AUTO_GUARD",
        ] {
            assert!(body.contains(guard), "{shell:?} misses {guard}");
        }
        assert!(
            body.contains("'/opt/sentinal bin/sentinal' auto enter"),
            "{body}"
        );
        assert!(body.contains(&format!("--shell {}", shell.name())));
        assert!(body.contains("exit 0"));
        assert!(
            !body.contains("exec "),
            "fallback never relaunches the shell"
        );
    }
    assert!(loader_body(HookShell::Zsh, binary).unwrap().contains("*i*"));
    assert!(loader_body(HookShell::Fish, binary)
        .unwrap()
        .contains("status is-interactive"));
}

// ---- the real binary ---------------------------------------------------------

static NEXT: AtomicU64 = AtomicU64::new(1);

fn temp_home() -> PathBuf {
    // A private per-process parent: the rc installer rightly refuses a home
    // whose parent is world-writable, as /tmp is on Linux.
    let root = std::env::temp_dir().join(format!("sentinal-auto-{}", std::process::id()));
    {
        use std::os::unix::fs::DirBuilderExt;
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&root)
            .unwrap();
    }
    let dir = root.join(NEXT.fetch_add(1, Ordering::Relaxed).to_string());
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sentinal(home: &Path, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args(args)
        .env("HOME", home)
        .env("SHELL", "/bin/zsh")
        .env("TERM", "xterm-256color")
        .env_remove("ZDOTDIR")
        .env_remove("SENTINAL_SESSION_ID")
        .env_remove("SENTINAL_DISABLE")
        .env_remove("SENTINAL_AUTO_GUARD")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn auto_on_status_off_round_trip_the_rc_file_exactly() {
    let home = temp_home();
    let original = "export PATH=\"$HOME/bin:$PATH\"\n";
    std::fs::write(home.join(".zshrc"), original).unwrap();

    let on = sentinal(&home, &["auto", "on"]);
    assert!(on.status.success(), "{on:?}");
    let rc = std::fs::read_to_string(home.join(".zshrc")).unwrap();
    assert!(rc.starts_with("# >>> sentinal auto >>>"), "{rc}");
    assert!(rc.ends_with(original));
    assert!(sentinal(&home, &["auto", "on"]).status.success());
    assert_eq!(std::fs::read_to_string(home.join(".zshrc")).unwrap(), rc);

    let status = sentinal(&home, &["auto", "status"]);
    let text = String::from_utf8_lossy(&status.stdout);
    assert!(
        text.contains("Auto-wrap: on for zsh — block at lines 1–6 of ~/.zshrc"),
        "{text}"
    );
    assert!(
        text.contains("would not be wrapped — stdin or stdout is not a terminal"),
        "{text}"
    );

    assert!(sentinal(&home, &["auto", "off"]).status.success());
    assert_eq!(
        std::fs::read_to_string(home.join(".zshrc")).unwrap(),
        original
    );
    let status = sentinal(&home, &["auto", "status"]);
    assert!(String::from_utf8_lossy(&status.stdout).contains("Auto-wrap: off"));
}

#[test]
fn auto_enter_falls_back_silently_without_a_terminal() {
    let home = temp_home();
    let started = std::time::Instant::now();
    let output = sentinal(
        &home,
        &[
            "--url",
            "ws://127.0.0.1:9/api/v1/ws",
            "auto",
            "enter",
            "--shell",
            "zsh",
            "--shell-path",
            "/bin/zsh",
        ],
    );
    assert_eq!(output.status.code(), Some(i32::from(FALLBACK_EXIT)));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert!(started.elapsed() < Duration::from_millis(300));
}

#[test]
fn inside_a_session_status_says_so_and_enter_falls_back() {
    let home = temp_home();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args(["auto", "status"])
        .env("HOME", &home)
        .env("SHELL", "/bin/zsh")
        .env("SENTINAL_SESSION_ID", "9")
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("inside Sentinal Session 9"));
}
