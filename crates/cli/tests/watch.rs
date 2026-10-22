//! `sentinal watch` parsing, planning, and fake-daemon behavior.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use clap::Parser;
use cli::{
    args::{Cli, Command},
    client::{ClientError, DaemonClient},
    watch::{create_session, plan_watch, WatchContext, WatchError, WatchPlan, SESSION_ENV},
};
use futures_util::{SinkExt, StreamExt};
use protocol::{
    ClientRequest, DaemonMessage, Envelope, MachineId, ProtocolError, Session, SessionId,
    SessionOrigin, Timestamp,
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-cli-watch-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn parse(args: &[&str]) -> Command {
    Cli::try_parse_from(args).unwrap().command
}

fn context() -> WatchContext {
    WatchContext {
        cwd: PathBuf::from("/work/my project"),
        size: Some((181, 47)),
        shell: Some("/bin/zsh".into()),
        existing_session: None,
        interactive: true,
        env: Some(vec![("PATH".into(), "/opt/homebrew/bin:/usr/bin".into())]),
    }
}

// ---- parsing -------------------------------------------------------------

#[test]
fn bare_watch_has_no_title_or_command() {
    assert!(matches!(
        parse(&["sentinal", "watch"]),
        Command::Watch {
            title: None,
            command: None
        }
    ));
    // A trailing separator with nothing after it is still a bare Watch.
    assert!(matches!(
        parse(&["sentinal", "watch", "--"]),
        Command::Watch { command: None, .. }
    ));
}

#[test]
fn title_and_command_after_separator_are_preserved_exactly() {
    let args = [
        "sentinal",
        "watch",
        "-t",
        "Qwen Training",
        "--",
        "python",
        "-c",
        "print('a b'); import os",
        "  two  spaces ",
        "\"quoted\"",
        "$(id) `id` ; | & > <",
        "日本語 🚀",
        "",
        "--",
        "-t",
    ];
    let Command::Watch { title, command } = parse(&args) else {
        panic!("expected watch");
    };
    assert_eq!(title.as_deref(), Some("Qwen Training"));
    let expected: Vec<String> = args[5..].iter().map(|arg| (*arg).to_string()).collect();
    assert_eq!(command, Some(expected));
}

#[test]
fn command_without_separator_is_rejected() {
    assert!(Cli::try_parse_from(["sentinal", "watch", "python", "train.py"]).is_err());
}

#[test]
fn existing_commands_still_parse_under_the_sentinal_name() {
    assert!(matches!(parse(&["sentinal", "ls"]), Command::List));
    assert!(matches!(
        parse(&["ta", "attach", "7"]),
        Command::Attach { id: 7 }
    ));
}

// ---- planning --------------------------------------------------------------

#[test]
fn plan_uses_current_cwd_dimensions_shell_and_watch_origin() {
    let command = vec!["python".to_string(), "long job.py".to_string()];
    let plan = plan_watch(Some("Job".into()), Some(command.clone()), &context()).unwrap();
    assert_eq!(
        plan,
        WatchPlan::Create(ClientRequest::CreateSession {
            shell: Some("/bin/zsh".into()),
            cwd: Some("/work/my project".into()),
            cols: 181,
            rows: 47,
            origin: SessionOrigin::Watch,
            title: Some("Job".into()),
            command: Some(command),
            env: Some(vec![("PATH".into(), "/opt/homebrew/bin:/usr/bin".into())]),
        })
    );
}

#[test]
fn plan_defaults_size_when_the_terminal_size_is_unknown() {
    let mut context = context();
    context.size = None;
    context.shell = Some(String::new());
    let WatchPlan::Create(ClientRequest::CreateSession {
        cols, rows, shell, ..
    }) = plan_watch(None, None, &context).unwrap()
    else {
        panic!("expected create");
    };
    assert_eq!((cols, rows, shell), (80, 24, None));
}

#[test]
fn plan_treats_a_zero_terminal_size_as_unknown() {
    for size in [(0, 0), (0, 40), (120, 0)] {
        let mut context = context();
        context.size = Some(size);
        let WatchPlan::Create(ClientRequest::CreateSession { cols, rows, .. }) =
            plan_watch(None, None, &context).unwrap()
        else {
            panic!("expected create");
        };
        assert_eq!((cols, rows), (80, 24), "size {size:?}");
    }
}

#[test]
fn nested_watch_creates_nothing_and_reports_refused_command() {
    let mut context = context();
    context.existing_session = Some("12".into());
    assert_eq!(
        plan_watch(None, None, &context).unwrap(),
        WatchPlan::AlreadyWatched {
            session: "12".into(),
            command_refused: false
        }
    );
    assert_eq!(
        plan_watch(None, Some(vec!["make".into()]), &context).unwrap(),
        WatchPlan::AlreadyWatched {
            session: "12".into(),
            command_refused: true
        }
    );
}

#[test]
fn empty_executable_and_non_terminal_are_rejected_before_contact() {
    let error = plan_watch(None, Some(vec![String::new()]), &context()).unwrap_err();
    assert!(matches!(
        error,
        WatchError::Invalid(ProtocolError::EmptyCommandExecutable)
    ));

    let mut context = context();
    context.interactive = false;
    assert!(matches!(
        plan_watch(None, Some(vec!["make".into()]), &context).unwrap_err(),
        WatchError::NotATerminal
    ));
}

// ---- fake daemon ---------------------------------------------------------

#[derive(Clone, Copy)]
enum Reply {
    Create,
    Reject,
    ObserveThenCreate,
}

struct FakeDaemon {
    url: String,
    token_file: PathBuf,
    connections: Arc<AtomicUsize>,
    requests: Arc<std::sync::Mutex<Vec<ClientRequest>>>,
}

async fn fake_daemon(reply: Reply) -> FakeDaemon {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}/api/v1/ws", listener.local_addr().unwrap());
    let token_file = temp_dir().join("token");
    std::fs::write(&token_file, "test-token\n").unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
    let (counter, log) = (connections.clone(), requests.clone());
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            let log = log.clone();
            tokio::spawn(async move {
                let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
                while let Some(Ok(Message::Text(text))) = socket.next().await {
                    let request = Envelope::<ClientRequest>::decode(text.as_str())
                        .unwrap()
                        .payload;
                    log.lock().unwrap().push(request.clone());
                    let message = match (reply, request) {
                        (
                            Reply::Create | Reply::ObserveThenCreate,
                            ClientRequest::CreateSession { cwd, title, .. },
                        ) => {
                            let mut session = Session::new(
                                SessionId(7),
                                MachineId("fake".into()),
                                "/bin/zsh",
                                cwd.unwrap_or_default(),
                                Timestamp(1),
                            );
                            session.origin = SessionOrigin::Watch;
                            session.title = title;
                            DaemonMessage::SessionUpdated { session }
                        }
                        _ => DaemonMessage::Error {
                            message: "unknown shell '/nope'".into(),
                        },
                    };
                    if matches!(reply, Reply::ObserveThenCreate) {
                        // An unsolicited observation of an adopted process arrives first.
                        let mut adopted = Session::new(
                            SessionId(77),
                            MachineId("fake".into()),
                            "",
                            "/",
                            Timestamp(1),
                        );
                        adopted.origin = SessionOrigin::Adopted;
                        adopted.capture = protocol::CaptureLevel::Process;
                        for observation in [
                            DaemonMessage::SessionUpdated { session: adopted },
                            DaemonMessage::SessionMetrics {
                                metrics: protocol::SessionMetrics {
                                    session_id: SessionId(77),
                                    at: Timestamp(1),
                                    totals: protocol::ProcessTotals::default(),
                                },
                            },
                        ] {
                            let text = Envelope::new(observation).encode().unwrap();
                            socket.send(Message::Text(text.into())).await.unwrap();
                        }
                    }
                    let text = Envelope::new(message).encode().unwrap();
                    socket.send(Message::Text(text.into())).await.unwrap();
                }
            });
        }
    });
    FakeDaemon {
        url,
        token_file,
        connections,
        requests,
    }
}

fn create_request() -> ClientRequest {
    match plan_watch(None, Some(vec!["make".into(), "all".into()]), &context()).unwrap() {
        WatchPlan::Create(request) => request,
        other => panic!("expected create, got {other:?}"),
    }
}

async fn connect(url: &str, token: &Path) -> Result<DaemonClient, ClientError> {
    DaemonClient::connect(&[url.to_string()], token).await
}

#[tokio::test]
async fn created_watch_session_is_returned_with_the_exact_request() {
    let daemon = fake_daemon(Reply::Create).await;
    let request = create_request();
    let (_client, session) =
        create_session(|| connect(&daemon.url, &daemon.token_file), request.clone())
            .await
            .unwrap();
    assert_eq!(session.id, SessionId(7));
    assert_eq!(session.origin, SessionOrigin::Watch);
    assert_eq!(*daemon.requests.lock().unwrap(), vec![request]);
}

#[tokio::test]
async fn unsolicited_observations_are_never_taken_as_the_reply() {
    let daemon = fake_daemon(Reply::ObserveThenCreate).await;
    let (_client, session) = create_session(
        || connect(&daemon.url, &daemon.token_file),
        create_request(),
    )
    .await
    .unwrap();
    assert_eq!(session.id, SessionId(7));
}

#[test]
fn observations_are_classified_by_type_and_capture() {
    use cli::client::is_observation;
    let mut full = Session::new(
        SessionId(1),
        MachineId("m".into()),
        "/bin/zsh",
        "/",
        Timestamp(1),
    );
    assert!(!is_observation(&DaemonMessage::SessionUpdated {
        session: full.clone()
    }));
    full.origin = SessionOrigin::ShellHook;
    full.capture = protocol::CaptureLevel::Commands;
    assert!(is_observation(&DaemonMessage::SessionUpdated {
        session: full
    }));
    assert!(!is_observation(&DaemonMessage::Error {
        message: "x".into()
    }));
    assert!(!is_observation(&DaemonMessage::Sessions {
        sessions: vec![]
    }));
}

#[tokio::test]
async fn rejected_creation_is_a_clear_error() {
    let daemon = fake_daemon(Reply::Reject).await;
    let error = create_session(
        || connect(&daemon.url, &daemon.token_file),
        create_request(),
    )
    .await
    .unwrap_err();
    assert!(matches!(&error, WatchError::Rejected(message) if message.contains("/nope")));
    assert!(error
        .to_string()
        .starts_with("sentinald rejected the Watch Session"));
}

#[tokio::test]
async fn unavailable_daemon_is_a_clear_error() {
    // Bind then drop to obtain a loopback port with no listener.
    let port = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let token = temp_dir().join("token");
    std::fs::write(&token, "t").unwrap();
    let url = format!("ws://127.0.0.1:{port}/api/v1/ws");
    let error = create_session(|| connect(&url, &token), create_request())
        .await
        .unwrap_err();
    assert!(matches!(error, WatchError::Unavailable(_)));
    assert!(error.to_string().starts_with("cannot reach sentinald"));
}

// ---- real binary without a terminal ----------------------------------------

fn run_binary(daemon: &FakeDaemon, env: &[(&str, &str)], marker: &Path) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_sentinal"));
    command
        .args(["--url", &daemon.url, "--token-file"])
        .arg(&daemon.token_file)
        .args(["watch", "--", "touch"])
        .arg(marker)
        .env_remove(SESSION_ENV)
        .stdin(std::process::Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().unwrap()
}

#[tokio::test]
async fn nested_watch_binary_makes_zero_requests_and_never_runs_the_command() {
    let daemon = fake_daemon(Reply::Create).await;
    let marker = temp_dir().join("ran");
    let output = run_binary(&daemon, &[(SESSION_ENV, "12")], &marker);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("already watched by Session 12"), "{stderr}");
    assert!(stderr.contains("the command was not run"), "{stderr}");
    assert_eq!(daemon.connections.load(Ordering::SeqCst), 0);
    assert!(!marker.exists());
}

#[tokio::test]
async fn watch_without_a_terminal_fails_before_contact_and_never_runs_the_command() {
    let daemon = fake_daemon(Reply::Create).await;
    let marker = temp_dir().join("ran");
    let output = run_binary(&daemon, &[], &marker);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("interactive terminal"), "{stderr}");
    assert!(stderr.contains("not run outside Sentinal"), "{stderr}");
    assert_eq!(daemon.connections.load(Ordering::SeqCst), 0);
    assert!(!marker.exists());
}
