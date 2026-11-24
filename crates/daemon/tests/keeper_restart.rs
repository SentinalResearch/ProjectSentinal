//! Terminals survive a daemon restart (T5.5): a real `sentinald`, a real
//! shell held by a real keeper, the daemon killed with SIGKILL mid-output
//! and started again.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::framing::{decode_terminal_frame, encode_terminal_frame};
use futures_util::{SinkExt, StreamExt};
use protocol::{
    ClientRequest, DaemonMessage, Envelope, Session, SessionId, SessionOrigin, SessionState,
};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, Message};

const TOKEN: &str = "keeper-restart-test-token-0123456789";

struct Daemon {
    child: Option<Child>,
    port: u16,
}

impl Daemon {
    fn start(dir: &Path) -> Self {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let config_dir = dir.join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("token"), TOKEN).unwrap();
        let config = dir.join("daemon.toml");
        std::fs::write(
            &config,
            format!(
                "bind = \"127.0.0.1:{port}\"\ndata_dir = \"{data}\"\ntoken_file = \"{token}\"\n\
                 auth_file = \"{auth}\"\nlocal_socket = \"{socket}\"\nconfig_dir = \"{config_dir}\"\n\
                 isolated = true\ndevelopment = true\n",
                data = dir.join("data").display(),
                token = config_dir.join("token").display(),
                auth = config_dir.join("auth.json").display(),
                socket = dir.join("s.sock").display(),
                config_dir = config_dir.display(),
            ),
        )
        .unwrap();
        let starts = std::fs::read_dir(dir).map_or(0, |entries| {
            entries
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("daemon-"))
                .count()
        });
        let log = std::fs::File::create(dir.join(format!("daemon-{starts}.log"))).unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_sentinald"))
            .env("TA_CONFIG", &config)
            .env("HOME", dir)
            .env("NO_COLOR", "1")
            .env_remove("SENTINAL_KEEPERS")
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap();
        let daemon = Daemon {
            child: Some(child),
            port,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(Instant::now() < deadline, "the daemon did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        daemon
    }

    /// SIGKILL: nothing closes, nothing is flushed.
    fn crash(&mut self) {
        if let Some(mut child) = self.child.take() {
            child.kill().unwrap();
            child.wait().unwrap();
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        self.crash();
    }
}

/// Ends every keeper a test's daemons started, however the test ends: a
/// crashed daemon rightly leaves them running.
struct Keepers(PathBuf);

impl Keepers {
    fn of(dir: &Path) -> Self {
        Keepers(daemon::keeper::paths::socket_dir(&dir.join("data")))
    }
}

impl Drop for Keepers {
    fn drop(&mut self) {
        for entry in std::fs::read_dir(&self.0).into_iter().flatten().flatten() {
            daemon::keeper::client::shut_down(&entry.path());
        }
        std::thread::sleep(Duration::from_millis(300));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(port: u16) -> Socket {
    let mut request = format!("ws://127.0.0.1:{port}/api/v1/ws")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "authorization",
        HeaderValue::from_str(&format!("Bearer {TOKEN}")).unwrap(),
    );
    tokio_tungstenite::connect_async(request).await.unwrap().0
}

async fn send(socket: &mut Socket, request: ClientRequest) {
    let text = Envelope::new(request).encode().unwrap();
    socket.send(Message::Text(text.into())).await.unwrap();
}

/// The next structured message `wanted` accepts, within a deadline.
async fn expect(socket: &mut Socket, wanted: impl Fn(&DaemonMessage) -> bool) -> DaemonMessage {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            if let Some(Ok(Message::Text(text))) = socket.next().await {
                let message = Envelope::<DaemonMessage>::decode(text.as_str())
                    .unwrap()
                    .payload;
                if wanted(&message) {
                    return message;
                }
            }
        }
    })
    .await
    .expect("the daemon answered")
}

/// Terminal output until `text` appears, within a deadline.
async fn output_until(socket: &mut Socket, session: SessionId, text: &str) -> String {
    let mut seen = Vec::new();
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if let Some(Ok(Message::Binary(frame))) = socket.next().await {
                let (id, bytes) = decode_terminal_frame(&frame).unwrap();
                if id == session {
                    seen.extend_from_slice(bytes);
                    if String::from_utf8_lossy(&seen).contains(text) {
                        return;
                    }
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("never saw {text:?} in {:?}", String::from_utf8_lossy(&seen)));
    String::from_utf8_lossy(&seen).into_owned()
}

async fn sessions(port: u16) -> Vec<Session> {
    let mut socket = connect(port).await;
    send(&mut socket, ClientRequest::ListSessions).await;
    match expect(&mut socket, |m| matches!(m, DaemonMessage::Sessions { .. })).await {
        DaemonMessage::Sessions { sessions } => sessions,
        _ => unreachable!(),
    }
}

async fn create_and_attach(port: u16, dir: &Path) -> (Socket, Session) {
    let mut socket = connect(port).await;
    send(
        &mut socket,
        ClientRequest::CreateSession {
            shell: Some("/bin/sh".into()),
            cwd: Some(dir.display().to_string()),
            cols: 80,
            rows: 24,
            origin: SessionOrigin::Ui,
            title: Some("kept".into()),
            command: None,
            env: None,
            sandbox: false,
        },
    )
    .await;
    let session = match expect(&mut socket, |m| {
        matches!(m, DaemonMessage::SessionUpdated { .. })
    })
    .await
    {
        DaemonMessage::SessionUpdated { session } => session,
        _ => unreachable!(),
    };
    attach(&mut socket, session.id).await;
    (socket, session)
}

async fn attach(socket: &mut Socket, id: SessionId) {
    send(
        socket,
        ClientRequest::Attach {
            session_id: id,
            history: true,
            cols: None,
            rows: None,
            mode: None,
        },
    )
    .await;
}

async fn type_line(socket: &mut Socket, id: SessionId, line: &str) {
    let frame = encode_terminal_frame(id, format!("{line}\r").as_bytes());
    socket.send(Message::Binary(frame.into())).await.unwrap();
}

fn temp_dir(name: &str) -> PathBuf {
    // Short: the keeper socket path must fit the platform's limit.
    let dir = std::env::temp_dir().join(format!("kr-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn log_text(dir: &Path, id: SessionId) -> String {
    let path = dir.join("data/sessions").join(format!("{}.log.zst", id.0));
    let mut text = String::new();
    let mut decoder = zstd::stream::read::Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    let _ = decoder.read_to_string(&mut text);
    text
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[tokio::test(flavor = "multi_thread")]
async fn a_terminal_survives_the_daemon_being_killed_mid_output() {
    let dir = temp_dir("survive");
    let _keepers = Keepers::of(&dir);
    let mut daemon = Daemon::start(&dir);
    let (mut socket, session) = create_and_attach(daemon.port, &dir).await;
    assert!(session.kept, "a new terminal is held by a keeper");
    let shell_pid = session.pid.unwrap();
    type_line(&mut socket, session.id, "echo before-$((40+2))").await;
    output_until(&mut socket, session.id, "before-42").await;
    // Output that arrives while no daemon is running.
    type_line(&mut socket, session.id, "sleep 2; echo during-$((40+3))").await;
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    drop(socket);
    daemon.crash();
    assert!(alive(shell_pid), "the shell outlives the daemon");
    tokio::time::sleep(Duration::from_secs(2)).await;

    let daemon = Daemon::start(&dir);
    let listed = sessions(daemon.port).await;
    let kept = listed.iter().find(|s| s.id == session.id).unwrap();
    assert!(
        !kept.state.is_finished(),
        "still live after the restart: {:?}",
        kept.state
    );
    assert!(kept.kept);
    assert_eq!(kept.pid, Some(shell_pid), "the same shell");

    let mut socket = connect(daemon.port).await;
    attach(&mut socket, session.id).await;
    // The screen has what happened before and during the restart…
    let screen = output_until(&mut socket, session.id, "during-43").await;
    assert!(screen.contains("before-42"), "{screen}");
    // …and typing reaches the same shell, once.
    type_line(&mut socket, session.id, "echo after-$((40+4))").await;
    output_until(&mut socket, session.id, "after-44").await;
    tokio::time::sleep(Duration::from_millis(1_500)).await;

    let log = log_text(&dir, session.id);
    for text in ["before-42", "during-43", "after-44"] {
        assert_eq!(
            log.matches(text).count(),
            1,
            "{text} once in the log:\n{log}"
        );
    }

    send(
        &mut socket,
        ClientRequest::Kill {
            session_id: session.id,
        },
    )
    .await;
    expect(&mut socket, |m| {
        matches!(m, DaemonMessage::SessionUpdated { session: s } if s.id == session.id && s.state.is_finished())
    })
    .await;
    let deadline = Instant::now() + Duration::from_secs(10);
    while alive(shell_pid) {
        assert!(Instant::now() < deadline, "the shell ended with Stop");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    drop(daemon);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_keeper_that_dies_ends_only_its_own_terminal() {
    let dir = temp_dir("crash");
    let _keepers = Keepers::of(&dir);
    let daemon = Daemon::start(&dir);
    let (mut one, first) = create_and_attach(daemon.port, &dir).await;
    let (_two, second) = create_and_attach(daemon.port, &dir).await;
    type_line(&mut one, first.id, "echo ready").await;
    output_until(&mut one, first.id, "ready").await;

    let store = store::Store::open(&dir.join("data/terminal-agent.sqlite3"))
        .or_else(|_| {
            let found = std::fs::read_dir(dir.join("data"))
                .unwrap()
                .flatten()
                .find(|entry| entry.path().extension().is_some_and(|e| e == "sqlite3"))
                .unwrap();
            store::Store::open(&found.path())
        })
        .unwrap();
    let keeper = store.keeper(first.id).unwrap().expect("a keeper row");
    Command::new("kill")
        .args(["-9", &keeper.keeper_pid.to_string()])
        .status()
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let listed = sessions(daemon.port).await;
        let state = |id| listed.iter().find(|s: &&Session| s.id == id).unwrap().state;
        if state(first.id) == SessionState::Interrupted {
            assert!(
                !state(second.id).is_finished(),
                "the other terminal goes on"
            );
            break;
        }
        assert!(Instant::now() < deadline, "the Session was not interrupted");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_keeper_that_does_not_match_its_record_is_not_reclaimed() {
    let dir = temp_dir("mismatch");
    let _keepers = Keepers::of(&dir);
    let mut daemon = Daemon::start(&dir);
    let (socket, session) = create_and_attach(daemon.port, &dir).await;
    drop(socket);
    daemon.crash();

    // Someone changed the record: the proof no longer matches.
    let path = std::fs::read_dir(dir.join("data"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|e| e == "sqlite3"))
        .unwrap();
    let store = store::Store::open(&path).unwrap();
    let mut keeper = store.keeper(session.id).unwrap().unwrap();
    keeper.secret_hex = "00".repeat(32);
    store.put_keeper(&keeper).unwrap();
    drop(store);

    let daemon = Daemon::start(&dir);
    let listed = sessions(daemon.port).await;
    let after = listed.iter().find(|s| s.id == session.id).unwrap();
    assert_eq!(after.state, SessionState::Interrupted, "fails closed");
    drop(daemon);
    // The keeper it could not claim is left alone; end it for the test.
    let _ = Command::new("kill")
        .args(["-9", &keeper.keeper_pid.to_string()])
        .status();
    let _ = std::fs::remove_dir_all(dir);
}

fn store_of(dir: &Path) -> store::Store {
    let path = std::fs::read_dir(dir.join("data"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|e| e == "sqlite3"))
        .unwrap();
    store::Store::open(&path).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_screen_program_comes_back_from_the_checkpoint_after_more_than_the_ring() {
    let dir = temp_dir("fullscreen");
    let _keepers = Keepers::of(&dir);
    let mut daemon = Daemon::start(&dir);
    let (mut socket, session) = create_and_attach(daemon.port, &dir).await;
    // 2 MB of output, more than the keeper's 1 MiB ring, then a full-screen
    // view that stays up.
    type_line(
        &mut socket,
        session.id,
        "i=0; while [ $i -lt 40000 ]; do echo bulk-line-$i-padding-padding; i=$((i+1)); done; printf '\\033[?1049h\\033[2J\\033[HFULLSCREEN-%s' APP; sleep 60",
    )
    .await;
    output_until(&mut socket, session.id, "FULLSCREEN-APP").await;
    // Past the 5 s checkpoint interval, with the screen settled.
    tokio::time::sleep(Duration::from_secs(7)).await;
    drop(socket);
    daemon.crash();

    let daemon = Daemon::start(&dir);
    let mut socket = connect(daemon.port).await;
    attach(&mut socket, session.id).await;
    let screen = output_until(&mut socket, session.id, "FULLSCREEN-APP").await;
    assert!(
        screen.contains("\u{1b}[?1049h"),
        "the viewer is on the full-screen view"
    );
    // The ring lost the start; the screen came from the keeper's checkpoint.
    let log = std::fs::read_to_string(dir.join("daemon-1.log")).unwrap();
    let plain: String = strip_ansi(&log);
    assert!(
        plain.contains("checkpoint=true"),
        "the screen came from a checkpoint:\n{plain}"
    );
    drop(daemon);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_keeper_from_another_version_is_told_to_stop_and_its_terminal_ends() {
    use daemon::keeper::protocol::{self as wire, Kind};
    let dir = temp_dir("version");
    let _keepers = Keepers::of(&dir);
    let mut daemon = Daemon::start(&dir);
    let (socket, session) = create_and_attach(daemon.port, &dir).await;
    drop(socket);
    daemon.crash();

    // Replace the keeper with one that speaks a protocol from the future.
    let keeper = store_of(&dir).keeper(session.id).unwrap().unwrap();
    Command::new("kill")
        .args(["-9", &keeper.keeper_pid.to_string()])
        .status()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let _ = std::fs::remove_file(&keeper.socket_path);
    let listener = std::os::unix::net::UnixListener::bind(&keeper.socket_path).unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&keeper.socket_path, std::fs::Permissions::from_mode(0o600))
            .unwrap();
    }
    let told = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let hello = wire::Hello {
            protocol: 99,
            keeper_version: "9.9.9".into(),
            session_id: keeper.session_id,
            keeper_pid: keeper.keeper_pid,
            shell_pid: keeper.shell_pid,
            nonce: "00".into(),
        };
        wire::write_frame(
            &mut stream,
            Kind::Hello,
            &serde_json::to_vec(&hello).unwrap(),
        )
        .unwrap();
        wire::read_frame(&mut stream, wire::MAX_FRAME)
            .ok()
            .flatten()
            .map(|frame| frame.kind)
    });

    let daemon = Daemon::start(&dir);
    assert_eq!(told.join().unwrap(), Some(Kind::Shutdown));
    let listed = sessions(daemon.port).await;
    let after = listed.iter().find(|s| s.id == session.id).unwrap();
    assert_eq!(after.state, SessionState::Interrupted);
    drop(daemon);
    let _ = std::fs::remove_dir_all(dir);
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for next in chars.by_ref() {
                if next.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_daemon_on_the_same_data_changes_nothing() {
    // Found by hand: a second start reclaimed the running daemon's keepers
    // and then stopped on the single-instance check, leaving them cut off.
    let dir = temp_dir("second");
    let _keepers = Keepers::of(&dir);
    let daemon = Daemon::start(&dir);
    let (mut socket, session) = create_and_attach(daemon.port, &dir).await;
    type_line(&mut socket, session.id, "echo first-$((1+1))").await;
    output_until(&mut socket, session.id, "first-2").await;

    let second = Command::new(env!("CARGO_BIN_EXE_sentinald"))
        .env("TA_CONFIG", dir.join("daemon.toml"))
        .env("HOME", &dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(!second.success(), "the second daemon refuses to start");

    type_line(&mut socket, session.id, "echo still-$((2+1))").await;
    output_until(&mut socket, session.id, "still-3").await;
    let listed = sessions(daemon.port).await;
    let after = listed.iter().find(|s| s.id == session.id).unwrap();
    assert!(!after.state.is_finished());
    drop(daemon);
    let _ = std::fs::remove_dir_all(dir);
}
