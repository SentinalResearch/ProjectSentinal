//! Hook reporter transport, deadline, silence, and suppression.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::hook_report::{
    bounded_text, parse_epoch, send, socket_path, suppressed, HookReport, ReportError, ReportKind,
    REPORT_DEADLINE,
};
use protocol::{
    Command, CommandId, Envelope, LocalRequest, LocalResponse, MachineId, Session, SessionId,
    SessionOrigin, Timestamp, MAX_COMMAND_TEXT_BYTES,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn short_dir() -> PathBuf {
    // Short: macOS socket paths must stay under 104 bytes.
    let dir = std::env::temp_dir().join(format!(
        "shr{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[derive(Clone, Copy)]
enum Mode {
    Answer,
    Silent,
    Garbage,
    Oversized,
}

struct Fake {
    socket: PathBuf,
    connections: Arc<AtomicUsize>,
    requests: Arc<Mutex<Vec<LocalRequest>>>,
}

fn answer(request: &LocalRequest) -> LocalResponse {
    match request {
        LocalRequest::RegisterShell {
            pid, shell, cwd, ..
        } => {
            let mut session = Session::new(
                SessionId(9),
                MachineId("fake".into()),
                shell.clone(),
                cwd.clone(),
                Timestamp(1),
            );
            session.origin = SessionOrigin::ShellHook;
            session.pid = Some(*pid);
            LocalResponse::Session { session }
        }
        _ => LocalResponse::Command {
            command: Command::start(
                CommandId(1),
                SessionId(9),
                MachineId("fake".into()),
                "x",
                "/",
                Timestamp(1),
            ),
        },
    }
}

fn fake(mode: Mode) -> Fake {
    let socket = short_dir().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (count, log) = (connections.clone(), requests.clone());
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            count.fetch_add(1, Ordering::SeqCst);
            let log = log.clone();
            tokio::spawn(async move {
                let mut header = [0_u8; 4];
                stream.read_exact(&mut header).await.unwrap();
                let mut body = vec![0; u32::from_be_bytes(header) as usize];
                stream.read_exact(&mut body).await.unwrap();
                let request = Envelope::<LocalRequest>::decode(std::str::from_utf8(&body).unwrap())
                    .unwrap()
                    .payload;
                log.lock().unwrap().push(request.clone());
                let reply = match mode {
                    Mode::Silent => {
                        std::future::pending::<()>().await;
                        return;
                    }
                    Mode::Garbage => b"not json".to_vec(),
                    Mode::Oversized => {
                        stream.write_all(&u32::MAX.to_be_bytes()).await.unwrap();
                        return;
                    }
                    Mode::Answer => Envelope::new(answer(&request))
                        .encode()
                        .unwrap()
                        .into_bytes(),
                };
                let length = u32::try_from(reply.len()).unwrap();
                stream.write_all(&length.to_be_bytes()).await.unwrap();
                stream.write_all(&reply).await.unwrap();
            });
        }
    });
    Fake {
        socket,
        connections,
        requests,
    }
}

fn report(kind: ReportKind) -> HookReport {
    HookReport {
        kind,
        pid: 4242,
        shell: "zsh".into(),
        sequence: 3,
        exit_code: 2,
        at: Timestamp(1_790_000_000_123),
        text: "grep -r 'a;b' $(pwd)".into(),
        cwd: "/work".into(),
        title: None,
    }
}

// ---- transport -------------------------------------------------------------

#[tokio::test]
async fn start_registers_then_reports_exact_fields() {
    let fake = fake(Mode::Answer);
    send(&fake.socket, &report(ReportKind::Start), REPORT_DEADLINE)
        .await
        .unwrap();
    let requests = fake.requests.lock().unwrap().clone();
    assert_eq!(
        requests,
        vec![
            LocalRequest::RegisterShell {
                pid: 4242,
                shell: "zsh".into(),
                cwd: "/work".into(),
                title: None,
            },
            LocalRequest::ShellCommandStarted {
                session_id: SessionId(9),
                pid: 4242,
                sequence: 3,
                text: "grep -r 'a;b' $(pwd)".into(),
                cwd: "/work".into(),
                started_at: Timestamp(1_790_000_000_123),
            },
        ]
    );
}

#[tokio::test]
async fn finish_reports_the_exit_status_and_time() {
    let fake = fake(Mode::Answer);
    send(&fake.socket, &report(ReportKind::Finish), REPORT_DEADLINE)
        .await
        .unwrap();
    assert_eq!(
        fake.requests.lock().unwrap()[1],
        LocalRequest::ShellCommandFinished {
            session_id: SessionId(9),
            pid: 4242,
            sequence: 3,
            exit_code: 2,
            finished_at: Timestamp(1_790_000_000_123),
        }
    );
}

#[tokio::test]
async fn a_missing_socket_fails_without_waiting() {
    let socket = short_dir().join("absent.sock");
    let error = send(&socket, &report(ReportKind::Register), REPORT_DEADLINE)
        .await
        .unwrap_err();
    assert!(matches!(error, ReportError::Io(_)), "{error}");
}

#[tokio::test(start_paused = true)]
async fn a_daemon_that_never_answers_hits_the_50ms_deadline() {
    let fake = fake(Mode::Silent);
    let started = tokio::time::Instant::now();
    let error = send(&fake.socket, &report(ReportKind::Start), REPORT_DEADLINE)
        .await
        .unwrap_err();
    assert!(matches!(error, ReportError::Timeout));
    assert_eq!(started.elapsed(), REPORT_DEADLINE);
}

#[tokio::test]
async fn malformed_and_oversized_replies_are_errors_not_panics() {
    for mode in [Mode::Garbage, Mode::Oversized] {
        let fake = fake(mode);
        let result = send(&fake.socket, &report(ReportKind::Start), REPORT_DEADLINE).await;
        assert!(result.is_err());
        assert_eq!(fake.connections.load(Ordering::SeqCst), 1);
    }
}

// ---- helpers ---------------------------------------------------------------

#[test]
fn epoch_times_parse_with_either_decimal_separator() {
    assert_eq!(
        parse_epoch("1790000000.123456"),
        Some(Timestamp(1_790_000_000_123))
    );
    assert_eq!(
        parse_epoch("1790000000,5"),
        Some(Timestamp(1_790_000_000_500))
    );
    assert_eq!(
        parse_epoch("1790000000"),
        Some(Timestamp(1_790_000_000_000))
    );
    assert_eq!(parse_epoch(""), None);
    assert_eq!(parse_epoch("soon"), None);
}

#[test]
fn oversized_text_is_truncated_at_a_character_boundary() {
    let text = "é".repeat(MAX_COMMAND_TEXT_BYTES);
    let bounded = bounded_text(text);
    assert!(bounded.len() <= MAX_COMMAND_TEXT_BYTES);
    assert!(bounded.chars().all(|c| c == 'é'));
}

#[test]
fn suppression_and_socket_selection() {
    assert!(suppressed(Some("7"), None));
    assert!(suppressed(None, Some("1")));
    assert!(!suppressed(Some(""), Some("0")));
    assert_eq!(
        socket_path(Some("/x/s.sock".into()), Some(Path::new("/h")), None),
        Some(PathBuf::from("/x/s.sock"))
    );
    assert_eq!(
        socket_path(Some("relative".into()), Some(Path::new("/h")), None),
        Some(PathBuf::from(
            "/h/.local/share/terminal-agent/run/sentinald.sock"
        ))
    );
}

#[test]
fn installed_config_socket_wins_over_the_runtime_default() {
    // The installer pins `local_socket` in daemon.toml; the CLI must use the
    // daemon's socket even when XDG_RUNTIME_DIR would point elsewhere.
    let home = short_dir();
    let config = home.join(".config/terminal-agent");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("daemon.toml"),
        "bind = \"127.0.0.1:7420\"\nlocal_socket = \"/pinned/run/sentinald.sock\"\n",
    )
    .unwrap();
    assert_eq!(
        socket_path(None, Some(&home), Some(Path::new("/run/user/1000"))),
        Some(PathBuf::from("/pinned/run/sentinald.sock"))
    );
    // SENTINAL_SOCKET still overrides the config.
    assert_eq!(
        socket_path(Some("/x/s.sock".into()), Some(&home), None),
        Some(PathBuf::from("/x/s.sock"))
    );
    // A relative configured path is ignored.
    std::fs::write(
        config.join("daemon.toml"),
        "local_socket = \"run/s.sock\"\n",
    )
    .unwrap();
    assert_eq!(
        socket_path(None, Some(&home), None),
        Some(home.join(".local/share/terminal-agent/run/sentinald.sock"))
    );
    let _ = std::fs::remove_dir_all(home);
}

// ---- the real binary -------------------------------------------------------

async fn run_reporter(socket: &Path, env: &[(&str, &str)]) -> std::process::Output {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_sentinal"));
    command
        .args([
            "hook", "report", "start", "--pid", "4242", "--shell", "zsh", "--seq", "1",
        ])
        .env("SENTINAL_SOCKET", socket)
        .env("SENTINAL_HOOK_TEXT", "echo secret-arg")
        .env("HOME", short_dir())
        .env_remove("SENTINAL_SESSION_ID")
        .env_remove("SENTINAL_DISABLE");
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().await.unwrap()
}

#[tokio::test]
async fn reporter_is_silent_and_successful_when_the_daemon_is_down() {
    let output = run_reporter(&short_dir().join("absent.sock"), &[]).await;
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

#[tokio::test]
async fn reporter_is_silent_on_malformed_replies() {
    let fake = fake(Mode::Garbage);
    let output = run_reporter(&fake.socket, &[]).await;
    assert!(output.status.success());
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

#[tokio::test]
async fn reporter_sends_text_from_the_environment() {
    let fake = fake(Mode::Answer);
    let output = run_reporter(&fake.socket, &[]).await;
    assert!(output.status.success() && output.stderr.is_empty());
    let requests = fake.requests.lock().unwrap().clone();
    assert!(matches!(
        &requests[1],
        LocalRequest::ShellCommandStarted { text, .. } if text == "echo secret-arg"
    ));
}

#[tokio::test]
async fn suppression_variables_make_zero_socket_connections() {
    for env in [("SENTINAL_SESSION_ID", "12"), ("SENTINAL_DISABLE", "1")] {
        let fake = fake(Mode::Answer);
        let output = run_reporter(&fake.socket, &[env]).await;
        assert!(output.status.success() && output.stderr.is_empty());
        assert_eq!(fake.connections.load(Ordering::SeqCst), 0, "{env:?}");
    }
}
