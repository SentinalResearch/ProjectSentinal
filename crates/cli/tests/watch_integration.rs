//! Real `sentinald` + `sentinal watch` in a pseudo-terminal, then reattach.
//!
//! Run with: `cargo build -p daemon --bin sentinald && cargo test -p cli --test
//! watch_integration -- --ignored --nocapture`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::client::DaemonClient;
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use protocol::{ClientRequest, DaemonMessage, Session, SessionOrigin};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

struct Terminal {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: mpsc::Receiver<Vec<u8>>,
    seen: String,
}

impl Terminal {
    fn spawn(program: &Path, args: &[String], env: &[(&str, &str)]) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(program);
        command.args(args);
        for (key, value) in env {
            command.env(key, value);
        }
        command.env_remove("SENTINAL_SESSION_ID");
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().unwrap();
        let writer = pair.master.take_writer().unwrap();
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                    break;
                }
            }
        });
        Terminal {
            master: pair.master,
            child,
            writer,
            output,
            seen: String::new(),
        }
    }

    fn wait_for(&mut self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.seen.contains(needle) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(remaining) {
                Ok(bytes) => self.seen.push_str(&String::from_utf8_lossy(&bytes)),
                Err(_) => panic!("timed out waiting for {needle:?}; saw:\n{}", self.seen),
            }
        }
    }
}

fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn process_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

async fn sessions(url: &str, token: &Path) -> Vec<Session> {
    let mut client = DaemonClient::connect(url, token).await.unwrap();
    match client.request(ClientRequest::ListSessions).await.unwrap() {
        DaemonMessage::Sessions { sessions } => sessions,
        other => panic!("unexpected {other:?}"),
    }
}

fn last_tick(text: &str) -> u32 {
    text.rsplit("tick-")
        .next()
        .and_then(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .unwrap_or(0)
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "starts a real sentinald, shell, and pseudo-terminals"]
async fn watch_integration() {
    let sentinal = PathBuf::from(env!("CARGO_BIN_EXE_sentinal"));
    let sentinald = sentinal.with_file_name("sentinald");
    assert!(
        sentinald.exists(),
        "build the daemon first: cargo build -p daemon --bin sentinald"
    );
    let shell = ["/bin/zsh", "/bin/bash", "/usr/bin/zsh", "/usr/bin/bash"]
        .into_iter()
        .find(|path| Path::new(path).exists())
        .expect("zsh or bash is required");

    let root = std::env::temp_dir().join(format!("swi-{}", std::process::id()));
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let port = free_port();
    let url = format!("ws://127.0.0.1:{port}/api/v1/ws");
    let token = root.join("token");
    let config = root.join("daemon.toml");
    std::fs::write(
        &config,
        format!(
            "bind = \"127.0.0.1:{port}\"\ndata_dir = \"{}\"\ntoken_file = \"{}\"\nauth_file = \"{}\"\n",
            root.join("data").display(),
            token.display(),
            root.join("auth.json").display()
        ),
    )
    .unwrap();
    let mut daemon = Command::new(&sentinald)
        .env("TA_CONFIG", &config)
        .env("HOME", &home)
        .env("SHELL", shell)
        .env_remove("SENTINAL_SESSION_ID")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    wait_until("sentinald to listen", || {
        token.exists() && std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
    });

    let common = vec![
        "--url".to_string(),
        url.clone(),
        "--token-file".to_string(),
        token.display().to_string(),
    ];
    let env = [
        ("HOME", home.to_str().unwrap()),
        ("SHELL", shell),
        ("TERM", "xterm-256color"),
    ];

    // 1. Watch a long job from an ordinary terminal.
    let job =
        "echo job-start; i=0; while [ $i -lt 3000 ]; do echo tick-$i; i=$((i+1)); sleep 0.1; done";
    let mut watch_args = common.clone();
    watch_args.extend(
        ["watch", "-t", "Integration Job", "--", "sh", "-c", job]
            .iter()
            .map(|arg| (*arg).to_string()),
    );
    let mut first = Terminal::spawn(&sentinal, &watch_args, &env);
    first.wait_for("tick-5");

    let listed = sessions(&url, &token).await;
    assert_eq!(listed.len(), 1, "{listed:?}");
    let session = listed[0].clone();
    assert_eq!(session.origin, SessionOrigin::Watch);
    assert_eq!(session.title.as_deref(), Some("Integration Job"));
    let pid = session.pid.expect("watch session has a root pid");
    let before_close = last_tick(&first.seen);

    // 2. Close the window: kill the client and drop its terminal.
    first.child.kill().unwrap();
    let _ = first.child.wait();
    drop(first.writer);
    drop(first.master);

    // 3. The Session, its shell, and the job keep running.
    std::thread::sleep(Duration::from_millis(300));
    assert!(process_alive(pid), "shell pid {pid} died with the client");
    let after = sessions(&url, &token).await;
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].pid, Some(pid));
    assert!(!after[0].state.is_finished(), "{:?}", after[0].state);

    // 4. Reattach from a second terminal and see the same, still-running screen.
    let mut attach_args = common.clone();
    attach_args.extend(["attach".to_string(), session.id.0.to_string()]);
    let mut second = Terminal::spawn(&sentinal, &attach_args, &env);
    second.wait_for("tick-");
    let next = before_close + 5;
    second.wait_for(&format!("tick-{next}"));
    second.writer.write_all(b"\x1cd").unwrap();
    let status = second.child.wait().unwrap();
    assert!(status.success(), "attach exited with {status:?}");

    // 5. The initial Command was recorded exactly once.
    let mut client = DaemonClient::connect(&url, &token).await.unwrap();
    match client
        .request(ClientRequest::GetHistory {
            session_id: Some(session.id),
            limit: 10,
        })
        .await
        .unwrap()
    {
        DaemonMessage::History { commands } => {
            let job_commands: Vec<_> = commands
                .iter()
                .filter(|command| command.text.contains("job-start"))
                .collect();
            assert_eq!(job_commands.len(), 1, "{commands:?}");
            assert!(job_commands[0].finished_at.is_none());
        }
        other => panic!("unexpected {other:?}"),
    }

    // 6. Explicit cleanup.
    match client
        .request(ClientRequest::Kill {
            session_id: session.id,
        })
        .await
        .unwrap()
    {
        DaemonMessage::SessionUpdated { session } => assert!(session.state.is_finished()),
        other => panic!("unexpected {other:?}"),
    }
    let _ = Command::new("kill")
        .args(["-TERM", &daemon.id().to_string()])
        .status();
    let _ = daemon.wait();
    std::fs::remove_dir_all(&root).unwrap();
    eprintln!(
        "watch_integration: session {} pid {pid} survived close and reattached",
        session.id.0
    );
}
