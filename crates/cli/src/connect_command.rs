//! `sentinal connect`, `connection status`, `disconnect`, and `identity reset`.
//!
//! The pairing code is read from a hidden terminal prompt or, with
//! `--code-stdin`, from one bounded stdin line. It is never accepted in argv,
//! a URL, or the environment, and is never printed. The local daemon performs
//! the HTTPS exchange; this command only talks to its private socket.

use crate::hook_report::{exchange, socket_path};
use protocol::{
    link::{ConnectionReport, LocalLinkState},
    local::SOCKET_ENV,
    LocalErrorCode, LocalRequest, LocalResponse, Secret,
};
use std::{
    ffi::OsString,
    fs::File,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// Documented exit codes.
pub mod exit {
    /// Success.
    pub const OK: i32 = 0;
    /// Internal error, or not connected (`status`, `disconnect`).
    pub const INTERNAL: i32 = 1;
    /// Usage error, a code in argv, or no terminal for the prompt.
    pub const USAGE: i32 = 2;
    /// The control is unreachable or is not a control.
    pub const CONTROL_UNREACHABLE: i32 = 3;
    /// The code was rejected or pairing is locked; `status`: revoked.
    pub const CODE_REJECTED: i32 = 4;
    /// This Machine is already connected.
    pub const ALREADY_CONNECTED: i32 = 5;
    /// The local daemon is unavailable or the identity is corrupt.
    pub const LOCAL_DAEMON: i32 = 6;
}

const MAX_CODE_BYTES: usize = 64;
const PROMPT_ATTEMPTS: usize = 3;
const CONNECT_DEADLINE: Duration = Duration::from_secs(30);
const LOCAL_DEADLINE: Duration = Duration::from_secs(10);

/// Where the pairing code comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeSource {
    /// Hidden prompt on the controlling terminal.
    Prompt,
    /// One line from stdin (`--code-stdin`).
    Stdin,
}

/// Result of a hidden read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// A line was entered.
    Line(String),
    /// Ctrl-C, Ctrl-D, or end of input.
    Cancelled,
}

/// The minimal terminal surface needed for a hidden prompt.
pub trait HiddenTerminal {
    /// Turn echo (and line editing/signals) off, or restore it.
    fn set_echo(&mut self, on: bool) -> std::io::Result<()>;
    /// Read one byte; `None` at end of input.
    fn read_byte(&mut self) -> std::io::Result<Option<u8>>;
    /// Write prompt text.
    fn write_text(&mut self, text: &str) -> std::io::Result<()>;
}

/// Refuse any way of passing a pairing code on the command line.
///
/// Runs before argument parsing so a rejected value is never echoed.
pub fn screen_argv(args: &[OsString]) -> Result<(), String> {
    let strings: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    // The subcommand is the first argument after global options and their values.
    let mut index = 1;
    while let Some(argument) = strings.get(index) {
        match argument.as_str() {
            "--url" | "--token-file" => index += 2,
            option if option.starts_with("--url=") || option.starts_with("--token-file=") => {
                index += 1
            }
            _ => break,
        }
    }
    if strings.get(index).map(String::as_str) != Some("connect") {
        return Ok(());
    }
    let position = index;
    let refused = || {
        "the pairing code is never accepted on the command line; run `sentinal connect CONTROL_URL` \
         and type it at the hidden prompt, or pipe it with --code-stdin"
            .to_string()
    };
    let mut positionals = 0;
    let mut rest = strings[position + 1..].iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--code-stdin" | "-h" | "--help" => {}
            "--name" => {
                rest.next();
            }
            name if name.starts_with("--name=") => {}
            flag if flag.starts_with('-') => return Err(refused()),
            url => {
                positionals += 1;
                if positionals > 1 {
                    return Err(refused());
                }
                if url.contains(['?', '#', '@']) {
                    return Err(
                        "the control URL must not contain a query, fragment, or credentials; \
                         the pairing code is never part of a URL"
                            .into(),
                    );
                }
            }
        }
    }
    Ok(())
}

/// Read a line with echo off; echo is restored on every path.
pub fn read_hidden(
    terminal: &mut dyn HiddenTerminal,
    prompt: &str,
) -> std::io::Result<ReadOutcome> {
    terminal.write_text(prompt)?;
    terminal.set_echo(false)?;
    let result = read_line(terminal);
    let restored = terminal.set_echo(true);
    let _ = terminal.write_text("\n");
    let outcome = result?;
    restored?;
    Ok(outcome)
}

fn read_line(terminal: &mut dyn HiddenTerminal) -> std::io::Result<ReadOutcome> {
    let mut line = Vec::new();
    loop {
        match terminal.read_byte()? {
            None | Some(0x03) => return Ok(ReadOutcome::Cancelled),
            Some(0x04) if line.is_empty() => return Ok(ReadOutcome::Cancelled),
            Some(b'\r' | b'\n') => {
                return Ok(ReadOutcome::Line(
                    String::from_utf8_lossy(&line).into_owned(),
                ));
            }
            Some(0x7f | 0x08) => {
                line.pop();
            }
            Some(byte) if byte >= 0x20 => {
                if line.len() >= MAX_CODE_BYTES {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "pairing code is too long",
                    ));
                }
                line.push(byte);
            }
            Some(_) => {}
        }
    }
}

/// `/dev/tty` with echo toggled through `stty`.
pub struct Tty {
    tty: File,
    saved: Option<String>,
}

impl Tty {
    /// Open the controlling terminal.
    pub fn open() -> std::io::Result<Self> {
        Ok(Tty {
            tty: std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/tty")?,
            saved: None,
        })
    }

    fn stty(&self, args: &[&str]) -> std::io::Result<String> {
        let output = Command::new("stty")
            .args(args)
            .stdin(self.tty.try_clone()?)
            .output()?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(std::io::Error::other("stty failed"))
        }
    }
}

impl HiddenTerminal for Tty {
    fn set_echo(&mut self, on: bool) -> std::io::Result<()> {
        if on {
            if let Some(saved) = self.saved.take() {
                self.stty(&[&saved])?;
            }
            Ok(())
        } else {
            self.saved = Some(self.stty(&["-g"])?);
            // No echo, byte-at-a-time, Ctrl-C as a byte: we handle it and restore.
            self.stty(&["-echo", "-icanon", "-isig", "min", "1", "time", "0"])
                .map(|_| ())
        }
    }

    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        let mut byte = [0_u8; 1];
        match self.tty.read(&mut byte)? {
            0 => Ok(None),
            _ => Ok(Some(byte[0])),
        }
    }

    fn write_text(&mut self, text: &str) -> std::io::Result<()> {
        self.tty.write_all(text.as_bytes())?;
        self.tty.flush()
    }
}

impl Drop for Tty {
    fn drop(&mut self) {
        let _ = self.set_echo(true);
    }
}

/// The local daemon socket (honours `SENTINAL_SOCKET`).
pub fn default_socket() -> Option<PathBuf> {
    socket_path(
        std::env::var_os(SOCKET_ENV).map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from).as_deref(),
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .as_deref(),
    )
}

/// Pair this Machine with a control.
#[allow(clippy::too_many_arguments)]
pub async fn connect(
    socket: &Path,
    control_url: &str,
    name: Option<&str>,
    source: CodeSource,
    terminal: &mut dyn HiddenTerminal,
    stdin: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let machine_id = match local(socket, LocalRequest::GetIdentity, LOCAL_DEADLINE).await {
        Ok(LocalResponse::Identity { machine_id, .. }) => machine_id,
        Ok(other) => return failure(other, err),
        Err(code) => return report_local_error(code, err),
    };
    let _ = writeln!(out, "Machine ID: {machine_id}");
    let attempts = if source == CodeSource::Prompt {
        PROMPT_ATTEMPTS
    } else {
        1
    };
    for attempt in 1..=attempts {
        let code = match read_code(source, terminal, stdin) {
            Ok(Some(code)) => code,
            Ok(None) => {
                let _ = writeln!(err, "sentinal: cancelled; nothing was connected");
                return exit::USAGE;
            }
            Err(message) => {
                let _ = writeln!(err, "sentinal: {message}");
                return exit::USAGE;
            }
        };
        let request = LocalRequest::ConnectControl {
            control_url: control_url.to_string(),
            pairing_code: code,
            display_name: name.map(str::to_string),
        };
        match local(socket, request, CONNECT_DEADLINE).await {
            Ok(LocalResponse::Connection { connection }) => {
                let name = connection.display_name.as_deref().unwrap_or("this Machine");
                let _ = writeln!(out, "Connected as {name}");
                return exit::OK;
            }
            Ok(LocalResponse::Error {
                code: LocalErrorCode::CodeRejected,
                message,
            }) if attempt < attempts => {
                let _ = writeln!(err, "sentinal: {message}");
            }
            Ok(other) => return failure(other, err),
            Err(code) => return report_local_error(code, err),
        }
    }
    exit::CODE_REJECTED
}

/// Report this Machine's control link.
pub async fn status(socket: &Path, json: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let connection = match local(socket, LocalRequest::ConnectionStatus, LOCAL_DEADLINE).await {
        Ok(LocalResponse::Connection { connection }) => connection,
        Ok(other) => return failure(other, err),
        Err(code) => return report_local_error(code, err),
    };
    if json {
        let _ = serde_json::to_writer_pretty(&mut *out, &connection);
        let _ = writeln!(out);
    } else {
        describe(&connection, out);
    }
    match connection.state {
        LocalLinkState::Active => exit::OK,
        LocalLinkState::NotConnected => exit::INTERNAL,
        LocalLinkState::ControlUnreachable => exit::CONTROL_UNREACHABLE,
        LocalLinkState::Revoked => exit::CODE_REJECTED,
    }
}

/// Remove this Machine's control link.
pub async fn disconnect(socket: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match local(socket, LocalRequest::DisconnectControl, CONNECT_DEADLINE).await {
        Ok(LocalResponse::Connection { connection }) => {
            let _ = writeln!(
                out,
                "Disconnected; Machine ID {} is kept",
                connection.machine_id
            );
            if let Some(message) = connection.message {
                let _ = writeln!(err, "sentinal: warning: {message}");
            }
            exit::OK
        }
        Ok(other) => failure(other, err),
        Err(code) => report_local_error(code, err),
    }
}

/// Replace the Machine identity after an explicit confirmation.
pub async fn identity_reset(
    socket: &Path,
    yes: bool,
    stdin: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    if !yes {
        let _ = write!(
            out,
            "This gives this Machine a new ID and removes its control link; the old entry stays in the \
             control until you revoke it. Type RESET to continue: "
        );
        let _ = out.flush();
        let mut answer = String::new();
        if stdin.take(64).read_line(&mut answer).is_err() || answer.trim() != "RESET" {
            let _ = writeln!(err, "sentinal: identity unchanged");
            return exit::USAGE;
        }
    }
    match local(socket, LocalRequest::ResetIdentity, LOCAL_DEADLINE).await {
        Ok(LocalResponse::Identity { machine_id, .. }) => {
            let _ = writeln!(
                out,
                "New Machine ID: {machine_id}; restart sentinald to use it for new Sessions"
            );
            exit::OK
        }
        Ok(other) => failure(other, err),
        Err(code) => report_local_error(code, err),
    }
}

fn read_code(
    source: CodeSource,
    terminal: &mut dyn HiddenTerminal,
    stdin: &mut dyn BufRead,
) -> Result<Option<Secret>, String> {
    match source {
        CodeSource::Prompt => match read_hidden(terminal, "Pairing code: ") {
            Ok(ReadOutcome::Line(line)) => Ok(Some(Secret::new(line.trim()))),
            Ok(ReadOutcome::Cancelled) => Ok(None),
            Err(error) if error.kind() == std::io::ErrorKind::InvalidInput => {
                Err("the pairing code is too long".into())
            }
            Err(error) => Err(format!(
                "cannot read the pairing code from the terminal: {error}"
            )),
        },
        CodeSource::Stdin => {
            let mut line = Vec::new();
            stdin
                .take((MAX_CODE_BYTES + 2) as u64)
                .read_until(b'\n', &mut line)
                .map_err(|error| format!("cannot read the pairing code from stdin: {error}"))?;
            let text = String::from_utf8(line)
                .map_err(|_| "the pairing code is not valid text".to_string())?;
            let code = text.trim_end_matches(['\r', '\n']);
            if code.len() > MAX_CODE_BYTES {
                return Err("the pairing code on stdin is too long".into());
            }
            if code.trim().is_empty() {
                return Ok(None);
            }
            Ok(Some(Secret::new(code.trim())))
        }
    }
}

enum LocalCallError {
    Unavailable(String),
    Timeout,
}

async fn local(
    socket: &Path,
    request: LocalRequest,
    deadline: Duration,
) -> Result<LocalResponse, LocalCallError> {
    match tokio::time::timeout(deadline, exchange(socket, request)).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(LocalCallError::Unavailable(format!(
            "cannot reach sentinald at {}: {error}",
            socket.display()
        ))),
        Err(_) => Err(LocalCallError::Timeout),
    }
}

fn report_local_error(error: LocalCallError, err: &mut dyn Write) -> i32 {
    let message = match error {
        LocalCallError::Unavailable(message) => message,
        LocalCallError::Timeout => "sentinald did not answer in time".into(),
    };
    let _ = writeln!(err, "sentinal: {message}");
    exit::LOCAL_DAEMON
}

fn failure(response: LocalResponse, err: &mut dyn Write) -> i32 {
    let (code, message) = match response {
        LocalResponse::Error { code, message } => (code, message),
        _ => (
            LocalErrorCode::Internal,
            "sentinald sent an unexpected reply".into(),
        ),
    };
    let _ = writeln!(err, "sentinal: {message}");
    match code {
        LocalErrorCode::CodeRejected => exit::CODE_REJECTED,
        LocalErrorCode::ControlUnreachable => exit::CONTROL_UNREACHABLE,
        LocalErrorCode::AlreadyConnected => exit::ALREADY_CONNECTED,
        LocalErrorCode::IdentityCorrupt | LocalErrorCode::Unavailable => exit::LOCAL_DAEMON,
        LocalErrorCode::InvalidRequest => exit::USAGE,
        _ => exit::INTERNAL,
    }
}

fn describe(connection: &ConnectionReport, out: &mut dyn Write) {
    let state = match connection.state {
        LocalLinkState::Active => "connected",
        LocalLinkState::NotConnected => "not connected",
        LocalLinkState::ControlUnreachable => "connected, but the control is unreachable",
        LocalLinkState::Revoked => "revoked by the control",
    };
    let _ = writeln!(out, "Machine ID:   {}", connection.machine_id);
    let _ = writeln!(out, "State:        {state}");
    if let Some(url) = &connection.control_url {
        let _ = writeln!(out, "Control:      {url}");
    }
    if let Some(name) = &connection.display_name {
        let _ = writeln!(out, "Display name: {name}");
    }
    if let Some(message) = &connection.message {
        let _ = writeln!(out, "Note:         {message}");
    }
}
