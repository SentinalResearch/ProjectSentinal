//! `sentinal connect`, `connection status`, `disconnect`, and `identity reset`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cli::connect_command::{
    self, exit, read_hidden, screen_argv, CodeSource, HiddenTerminal, ReadOutcome,
};
use protocol::{
    link::{ConnectionReport, LocalLinkState},
    Envelope, LocalErrorCode, LocalRequest, LocalResponse, MachineId,
};
use std::{
    collections::VecDeque,
    ffi::OsString,
    io::Cursor,
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

const MACHINE: &str = "01J9Z3K8V6Q2T7M4X5C0RNB8DA";
const CODE: &str = "7KQ4M-2XZ9C";

// ---- argv screening -------------------------------------------------------------

fn argv(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}

#[test]
fn every_command_line_code_form_is_refused_without_echo() {
    for form in [
        vec![
            "sentinal",
            "connect",
            "https://c.example.ts.net",
            "--code",
            CODE,
        ],
        vec![
            "sentinal",
            "connect",
            "https://c.example.ts.net",
            "--code=7KQ4M-2XZ9C",
        ],
        vec![
            "sentinal",
            "connect",
            "--pairing-code",
            CODE,
            "https://c.example.ts.net",
        ],
        vec![
            "sentinal",
            "connect",
            "https://c.example.ts.net",
            "-c",
            CODE,
        ],
        vec!["sentinal", "connect", "https://c.example.ts.net", CODE],
        vec![
            "sentinal",
            "connect",
            "https://c.example.ts.net/?code=7KQ4M-2XZ9C",
        ],
        vec![
            "sentinal",
            "connect",
            "https://c.example.ts.net#7KQ4M-2XZ9C",
        ],
        vec![
            "sentinal",
            "connect",
            "https://7KQ4M-2XZ9C@c.example.ts.net",
        ],
        vec![
            "sentinal",
            "--url",
            "x",
            "connect",
            "https://c.example.ts.net",
            "--code",
            CODE,
        ],
    ] {
        let message = screen_argv(&argv(&form)).unwrap_err();
        assert!(!message.contains("7KQ4M"), "{form:?} echoed: {message}");
    }
}

#[test]
fn valid_connect_forms_and_other_commands_pass_screening() {
    for form in [
        vec!["sentinal", "connect", "https://c.example.ts.net"],
        vec![
            "sentinal",
            "connect",
            "https://c.example.ts.net",
            "--name",
            "RTX PC",
        ],
        vec![
            "sentinal",
            "connect",
            "--name=RTX",
            "https://c.example.ts.net",
            "--code-stdin",
        ],
        vec!["sentinal", "connect", "--help"],
        vec!["sentinal", "ls"],
        vec!["sentinal", "adopt", "12", "-t", "--code"],
        vec!["sentinal", "watch", "-t", "connect", "--", "ls", "--code"],
    ] {
        assert!(screen_argv(&argv(&form)).is_ok(), "{form:?}");
    }
}

#[test]
fn the_real_binary_refuses_a_code_in_argv_without_printing_it() {
    let output = Command::new(env!("CARGO_BIN_EXE_sentinal"))
        .args(["connect", "https://c.example.ts.net", "--code=SECRETCODE1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(exit::USAGE));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!text.contains("SECRETCODE1"), "{text}");
    assert!(
        text.contains("never accepted on the command line"),
        "{text}"
    );
}

// ---- hidden input ---------------------------------------------------------------

#[derive(Default)]
struct FakeTerminal {
    input: VecDeque<u8>,
    echo_log: Vec<bool>,
    written: String,
    fail_read: bool,
}

impl FakeTerminal {
    fn typing(text: &[u8]) -> Self {
        FakeTerminal {
            input: text.iter().copied().collect(),
            ..FakeTerminal::default()
        }
    }
}

impl HiddenTerminal for FakeTerminal {
    fn set_echo(&mut self, on: bool) -> std::io::Result<()> {
        self.echo_log.push(on);
        Ok(())
    }

    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        if self.fail_read {
            return Err(std::io::Error::other("tty vanished"));
        }
        Ok(self.input.pop_front())
    }

    fn write_text(&mut self, text: &str) -> std::io::Result<()> {
        self.written.push_str(text);
        Ok(())
    }
}

#[test]
fn hidden_input_turns_echo_off_and_restores_it() {
    let mut terminal = FakeTerminal::typing(b"7KQ4M-2XZ9C\r");
    let outcome = read_hidden(&mut terminal, "Pairing code: ").unwrap();
    assert_eq!(outcome, ReadOutcome::Line(CODE.into()));
    assert_eq!(terminal.echo_log, vec![false, true]);
    assert!(terminal.written.starts_with("Pairing code: "));
    assert!(!terminal.written.contains("7KQ4M"), "typed code was echoed");
}

#[test]
fn backspace_edits_and_ctrl_c_cancels_with_echo_restored() {
    let mut terminal = FakeTerminal::typing(b"7KQ4X\x7fM-2XZ9C\n");
    assert_eq!(
        read_hidden(&mut terminal, "").unwrap(),
        ReadOutcome::Line(CODE.into())
    );

    let mut cancelled = FakeTerminal::typing(b"7KQ\x03");
    assert_eq!(
        read_hidden(&mut cancelled, "").unwrap(),
        ReadOutcome::Cancelled
    );
    assert_eq!(cancelled.echo_log, vec![false, true]);

    let mut eof = FakeTerminal::typing(b"");
    assert_eq!(read_hidden(&mut eof, "").unwrap(), ReadOutcome::Cancelled);
}

#[test]
fn hidden_input_is_bounded_and_read_errors_restore_echo() {
    let mut long = FakeTerminal::typing(&[b'A'; 100]);
    assert!(read_hidden(&mut long, "").is_err());
    assert_eq!(long.echo_log, vec![false, true]);

    let mut failing = FakeTerminal {
        fail_read: true,
        ..FakeTerminal::default()
    };
    assert!(read_hidden(&mut failing, "").is_err());
    assert_eq!(failing.echo_log, vec![false, true]);
}

// ---- against a fake local daemon ------------------------------------------------

static NEXT: AtomicU64 = AtomicU64::new(1);

struct Fake {
    socket: PathBuf,
    requests: Arc<Mutex<Vec<LocalRequest>>>,
}

/// Reply to each request kind from a script, in order.
fn fake_daemon(mut replies: Vec<LocalResponse>) -> Fake {
    let dir = std::env::temp_dir().join(format!(
        "scn{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let socket = dir.join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let log = requests.clone();
    replies.reverse();
    let replies = Arc::new(Mutex::new(replies));
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut header = [0_u8; 4];
            stream.read_exact(&mut header).await.unwrap();
            let mut body = vec![0; u32::from_be_bytes(header) as usize];
            stream.read_exact(&mut body).await.unwrap();
            log.lock().unwrap().push(
                Envelope::<LocalRequest>::decode(std::str::from_utf8(&body).unwrap())
                    .unwrap()
                    .payload,
            );
            let reply = replies.lock().unwrap().pop().expect("unexpected request");
            let text = Envelope::new(reply).encode().unwrap();
            stream
                .write_all(&u32::try_from(text.len()).unwrap().to_be_bytes())
                .await
                .unwrap();
            stream.write_all(text.as_bytes()).await.unwrap();
        }
    });
    Fake { socket, requests }
}

fn identity() -> LocalResponse {
    LocalResponse::Identity {
        machine_id: MachineId(MACHINE.into()),
        hostname: "pop-os".into(),
    }
}

fn report(state: LocalLinkState, name: Option<&str>) -> ConnectionReport {
    ConnectionReport {
        state,
        machine_id: MachineId(MACHINE.into()),
        control_url: Some("https://c.example.ts.net".into()),
        control_machine_id: None,
        display_name: name.map(Into::into),
        link_id: None,
        paired_at: None,
        last_heartbeat_at: None,
        message: None,
    }
}

fn connected(name: &str) -> LocalResponse {
    LocalResponse::Connection {
        connection: report(LocalLinkState::Active, Some(name)),
    }
}

fn error(code: LocalErrorCode) -> LocalResponse {
    LocalResponse::Error {
        code,
        message: format!("{code:?} detail"),
    }
}

async fn run_connect(
    fake: &Fake,
    source: CodeSource,
    terminal: &mut FakeTerminal,
    stdin: &[u8],
) -> (i32, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = connect_command::connect(
        &fake.socket,
        "https://c.example.ts.net",
        Some("RTX PC"),
        source,
        terminal,
        &mut Cursor::new(stdin.to_vec()),
        &mut out,
        &mut err,
    )
    .await;
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[tokio::test]
async fn interactive_connect_shows_the_id_then_connects() {
    let fake = fake_daemon(vec![identity(), connected("RTX PC")]);
    let mut terminal = FakeTerminal::typing(b"7kq4m-2xz9c\n");

    let (code, out, err) = run_connect(&fake, CodeSource::Prompt, &mut terminal, b"").await;

    assert_eq!(code, exit::OK, "{err}");
    assert!(out.contains(&format!("Machine ID: {MACHINE}")));
    assert!(out.contains("Connected as RTX PC"));
    assert!(!out.contains("7kq4m") && !err.contains("7kq4m"));
    let requests = fake.requests.lock().unwrap();
    assert_eq!(requests[0], LocalRequest::GetIdentity);
    let LocalRequest::ConnectControl {
        pairing_code,
        display_name,
        ..
    } = &requests[1]
    else {
        panic!("{:?}", requests[1])
    };
    assert_eq!(pairing_code.expose(), "7kq4m-2xz9c");
    assert_eq!(display_name.as_deref(), Some("RTX PC"));
}

#[tokio::test]
async fn a_rejected_code_can_be_retried_at_the_prompt() {
    let fake = fake_daemon(vec![
        identity(),
        error(LocalErrorCode::CodeRejected),
        connected("RTX PC"),
    ]);
    let mut terminal = FakeTerminal::typing(b"00000-00000\n7KQ4M-2XZ9C\n");
    let (code, _, err) = run_connect(&fake, CodeSource::Prompt, &mut terminal, b"").await;
    assert_eq!(code, exit::OK, "{err}");
    assert!(err.contains("CodeRejected detail"));
}

#[tokio::test]
async fn three_rejections_end_with_exit_four() {
    let fake = fake_daemon(vec![
        identity(),
        error(LocalErrorCode::CodeRejected),
        error(LocalErrorCode::CodeRejected),
        error(LocalErrorCode::CodeRejected),
    ]);
    let mut terminal = FakeTerminal::typing(b"a\nb\nc\n");
    let (code, _, _) = run_connect(&fake, CodeSource::Prompt, &mut terminal, b"").await;
    assert_eq!(code, exit::CODE_REJECTED);
}

#[tokio::test]
async fn code_stdin_reads_one_bounded_line_without_retries() {
    let fake = fake_daemon(vec![identity(), connected("RTX PC")]);
    let mut terminal = FakeTerminal::default();
    let (code, _, err) =
        run_connect(&fake, CodeSource::Stdin, &mut terminal, b"7KQ4M-2XZ9C\n").await;
    assert_eq!(code, exit::OK, "{err}");
    assert!(
        terminal.echo_log.is_empty(),
        "stdin path never touches the terminal"
    );

    let rejected = fake_daemon(vec![identity(), error(LocalErrorCode::CodeRejected)]);
    let (code, _, _) = run_connect(
        &rejected,
        CodeSource::Stdin,
        &mut terminal,
        b"7KQ4M-2XZ9C\n",
    )
    .await;
    assert_eq!(code, exit::CODE_REJECTED);

    let long = fake_daemon(vec![identity()]);
    let (code, _, err) = run_connect(&long, CodeSource::Stdin, &mut terminal, &[b'A'; 200]).await;
    assert_eq!(code, exit::USAGE);
    assert!(!err.contains("AAAA"));
}

#[tokio::test]
async fn every_daemon_failure_maps_to_its_documented_exit_code() {
    for (reply, expected) in [
        (
            LocalErrorCode::ControlUnreachable,
            exit::CONTROL_UNREACHABLE,
        ),
        (LocalErrorCode::AlreadyConnected, exit::ALREADY_CONNECTED),
        (LocalErrorCode::InvalidRequest, exit::USAGE),
        (LocalErrorCode::Internal, exit::INTERNAL),
    ] {
        let fake = fake_daemon(vec![identity(), error(reply)]);
        let (code, _, _) = run_connect(
            &fake,
            CodeSource::Stdin,
            &mut FakeTerminal::default(),
            b"7KQ4M-2XZ9C\n",
        )
        .await;
        assert_eq!(code, expected, "{reply:?}");
    }
    let corrupt = fake_daemon(vec![error(LocalErrorCode::IdentityCorrupt)]);
    let (code, _, _) = run_connect(
        &corrupt,
        CodeSource::Stdin,
        &mut FakeTerminal::default(),
        b"x\n",
    )
    .await;
    assert_eq!(code, exit::LOCAL_DAEMON);
}

#[tokio::test]
async fn an_absent_daemon_is_exit_six() {
    let fake = Fake {
        socket: std::env::temp_dir().join("no-such-sentinal.sock"),
        requests: Arc::default(),
    };
    let (code, _, err) = run_connect(
        &fake,
        CodeSource::Stdin,
        &mut FakeTerminal::default(),
        b"x\n",
    )
    .await;
    assert_eq!(code, exit::LOCAL_DAEMON);
    assert!(err.contains("sentinald"), "{err}");
}

#[tokio::test]
async fn status_and_disconnect_exit_codes() {
    for (state, expected) in [
        (LocalLinkState::Active, exit::OK),
        (LocalLinkState::NotConnected, exit::INTERNAL),
        (
            LocalLinkState::ControlUnreachable,
            exit::CONTROL_UNREACHABLE,
        ),
        (LocalLinkState::Revoked, exit::CODE_REJECTED),
    ] {
        let fake = fake_daemon(vec![LocalResponse::Connection {
            connection: report(state, Some("RTX PC")),
        }]);
        let mut out = Vec::new();
        let code = connect_command::status(&fake.socket, false, &mut out, &mut Vec::new()).await;
        assert_eq!(code, expected, "{state:?}");
    }

    let fake = fake_daemon(vec![LocalResponse::Connection {
        connection: report(LocalLinkState::Active, Some("RTX PC")),
    }]);
    let mut out = Vec::new();
    connect_command::status(&fake.socket, true, &mut out, &mut Vec::new()).await;
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(json["state"], "ACTIVE");

    let not_connected = fake_daemon(vec![error(LocalErrorCode::NotConnected)]);
    let code =
        connect_command::disconnect(&not_connected.socket, &mut Vec::new(), &mut Vec::new()).await;
    assert_eq!(code, exit::INTERNAL);
    let mut warned = report(LocalLinkState::NotConnected, None);
    warned.message = Some("the control was not told; revoke it".into());
    let done = fake_daemon(vec![LocalResponse::Connection { connection: warned }]);
    let mut err = Vec::new();
    let code = connect_command::disconnect(&done.socket, &mut Vec::new(), &mut err).await;
    assert_eq!(code, exit::OK);
    assert!(String::from_utf8(err).unwrap().contains("revoke"));
}

#[tokio::test]
async fn identity_reset_needs_confirmation() {
    let fake = fake_daemon(vec![identity()]);
    let mut out = Vec::new();
    let refused = connect_command::identity_reset(
        &fake.socket,
        false,
        &mut Cursor::new(b"no\n".to_vec()),
        &mut out,
        &mut Vec::new(),
    )
    .await;
    assert_eq!(refused, exit::USAGE);
    assert!(fake.requests.lock().unwrap().is_empty());

    let confirmed = connect_command::identity_reset(
        &fake.socket,
        false,
        &mut Cursor::new(b"RESET\n".to_vec()),
        &mut out,
        &mut Vec::new(),
    )
    .await;
    assert_eq!(confirmed, exit::OK);
    assert_eq!(
        fake.requests.lock().unwrap()[0],
        LocalRequest::ResetIdentity
    );
}
