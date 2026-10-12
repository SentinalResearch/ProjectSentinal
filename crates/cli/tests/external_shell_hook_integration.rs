//! Real zsh/bash/fish reporting to a real `sentinald` over the local socket.
//!
//! Run with: `cargo build -p daemon --bin sentinald && cargo test -p cli --test
//! external_shell_hook_integration -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use cli::client::DaemonClient;
use protocol::{
    CaptureLevel, ClientRequest, Command, DaemonMessage, Session, SessionOrigin, SessionState,
};
use std::path::Path;
use support::{find_shell, wait_until, Daemon, ShellTerm};

const OUTPUT_MARKER: &str = "COMMANDS-CAPTURE-MUST-NOT-STORE-THIS";

async fn request(daemon: &Daemon, request: ClientRequest) -> DaemonMessage {
    let mut client = DaemonClient::connect(std::slice::from_ref(&daemon.url), &daemon.token)
        .await
        .unwrap();
    client.request(request).await.unwrap()
}

async fn shell_session(daemon: &Daemon, pid: u32) -> Option<Session> {
    match request(daemon, ClientRequest::ListSessions).await {
        DaemonMessage::Sessions { sessions } => sessions
            .into_iter()
            .find(|session| session.pid == Some(pid) && session.origin == SessionOrigin::ShellHook),
        other => panic!("unexpected {other:?}"),
    }
}

async fn history(daemon: &Daemon, session: &Session) -> Vec<Command> {
    match request(
        daemon,
        ClientRequest::GetHistory {
            session_id: Some(session.id),
            limit: 50,
        },
    )
    .await
    {
        DaemonMessage::History { mut commands } => {
            commands.sort_by_key(|command| command.id);
            commands
        }
        other => panic!("unexpected {other:?}"),
    }
}

fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Handle::current().block_on(future)
}

fn exercise(daemon: &Daemon, shell: &str) {
    let sentinal = Path::new(env!("CARGO_BIN_EXE_sentinal"));
    let marker_file = support::short_dir("mk").join("out.txt");
    std::fs::write(&marker_file, OUTPUT_MARKER).unwrap();
    let mut term = ShellTerm::spawn(
        shell,
        sentinal,
        &[("SENTINAL_SOCKET", daemon.socket.display().to_string())],
    );
    let pid = term.pid();
    let metachar = "printf '%s\\n' 'a;b $(x) \"q\" | & >'";
    let cat = format!("cat {}", marker_file.display());
    for line in ["true", "false", "cd /tmp", metachar, &cat] {
        term.run(line);
    }

    let mut found = None;
    wait_until(&format!("{shell} Commands in sentinald"), || {
        found = block_on(shell_session(daemon, pid));
        found.as_ref().is_some_and(|session| {
            let commands = block_on(history(daemon, session));
            // Idle is its own update after the last Command finishes.
            session.state == SessionState::Idle
                && commands.len() == 5
                && commands.iter().all(|command| command.exit_code.is_some())
        })
    });
    let session = found.unwrap();
    assert_eq!(session.capture, CaptureLevel::Commands);
    assert_eq!(session.state, SessionState::Idle);

    let commands = block_on(history(daemon, &session));
    let summary: Vec<(&str, Option<i32>)> = commands
        .iter()
        .map(|command| (command.text.as_str(), command.exit_code))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("true", Some(0)),
            ("false", Some(1)),
            ("cd /tmp", Some(0)),
            (metachar, Some(0)),
            (cat.as_str(), Some(0)),
        ],
        "{shell}"
    );
    assert_eq!(commands[3].cwd, "/tmp");
    assert!(commands
        .iter()
        .all(|command| command.finished_at >= Some(command.started_at)));

    // A COMMANDS Session cannot be attached to.
    let refused = block_on(request(
        daemon,
        ClientRequest::Attach {
            session_id: session.id,
        },
    ));
    assert!(
        matches!(&refused, DaemonMessage::Error { message } if message.contains("COMMANDS")),
        "{refused:?}"
    );

    // Closing the terminal ends the Session with an unknown exit code.
    term.exit();
    wait_until(&format!("{shell} Session to end"), || {
        block_on(shell_session(daemon, pid))
            .is_some_and(|session| session.state == SessionState::Completed)
    });
    let ended = block_on(shell_session(daemon, pid)).unwrap();
    assert_eq!(ended.exit_code, None);
    eprintln!(
        "{shell}: Session {} recorded 5 Commands and ended",
        session.id.0
    );
}

fn assert_private(daemon: &Daemon) {
    let mut stored = Vec::new();
    for entry in walk(&daemon.data_dir) {
        stored.extend(std::fs::read(&entry).unwrap_or_default());
    }
    stored.extend(std::fs::read(&daemon.log).unwrap_or_default());
    let text = String::from_utf8_lossy(&stored);
    assert!(
        !text.contains(OUTPUT_MARKER),
        "terminal output was persisted"
    );
    assert!(!daemon.data_dir.join("audit.jsonl").exists());
    let logs = daemon.data_dir.join("sessions");
    assert!(
        !logs.exists() || std::fs::read_dir(&logs).unwrap().next().is_none(),
        "COMMANDS Sessions created a terminal log"
    );
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .flat_map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                walk(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "starts a real sentinald and real shells"]
async fn external_shell_hook_integration() {
    let daemon = Daemon::start(Path::new(env!("CARGO_BIN_EXE_sentinal")));
    let mut skipped = Vec::new();
    tokio::task::block_in_place(|| {
        for shell in ["zsh", "bash", "fish"] {
            if find_shell(shell).is_none() {
                skipped.push(shell);
                continue;
            }
            exercise(&daemon, shell);
        }
        assert_private(&daemon);
    });
    daemon.stop();
    eprintln!("skipped (not installed): {skipped:?}");
}
