//! The daemon's side of a keeper (`docs/R1_KEEPER_DESIGN.md` §5, §7, §9):
//! start one for a new Session, claim one after a restart, and present it
//! to the Session code as an ordinary PTY.

use super::protocol::{self as wire, Kind};
use super::KeeperRecord;
use crate::pty::{KeptPty, PtyError, PtySpawnRequest, SpawnedPty};
use protocol::SessionId;
use std::io::{BufRead, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long a new keeper may take to say it is ready.
const READY_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a keeper may take to answer a claim.
const CLAIM_TIMEOUT: Duration = Duration::from_secs(2);

/// Why a keeper could not be started or reclaimed. Each maps to the reason
/// its Session is interrupted (§8).
#[derive(Debug, thiserror::Error)]
pub enum KeeperError {
    /// No keeper answers at the recorded socket.
    #[error("no keeper answers: {0}")]
    Missing(String),
    /// A keeper answers but is not the one recorded.
    #[error("the keeper does not match its record: {0}")]
    Mismatch(&'static str),
    /// It speaks a protocol this daemon does not.
    #[error("the keeper speaks protocol {0}")]
    Version(u32),
    /// Anything else.
    #[error("keeper: {0}")]
    Io(#[from] std::io::Error),
    /// A message that is not what the protocol says.
    #[error("keeper: {0}")]
    Json(#[from] serde_json::Error),
}

/// What a claim brings back.
pub struct Claimed {
    /// The connection, now this daemon's.
    pub stream: UnixStream,
    /// Where the output stands.
    pub claimed: wire::Claimed,
    /// The screen as of an offset, if the keeper kept one.
    pub checkpoint: Option<(u64, Vec<u8>)>,
    /// Output lost while no daemon was connected.
    pub gap: Option<wire::Gap>,
    /// Kept output from `replay_from` to `claimed.output_end`.
    pub replay: Vec<u8>,
    /// Offset of the first replayed byte.
    pub replay_from: u64,
}

/// Connect to the keeper in `record`, check it is that keeper, and claim it.
/// `resume_from` is the offset this daemon has already processed.
pub fn claim(record: &KeeperRecord, resume_from: u64) -> Result<Claimed, KeeperError> {
    let secret = wire::unhex(&record.secret_hex).ok_or(KeeperError::Mismatch("secret"))?;
    if let Some(dir) = record.socket_path.parent() {
        super::paths::check_private(dir, 0o700)
            .map_err(|error| KeeperError::Missing(error.to_string()))?;
    }
    super::paths::check_private(&record.socket_path, 0o600)
        .map_err(|error| KeeperError::Missing(error.to_string()))?;
    let stream = UnixStream::connect(&record.socket_path)
        .map_err(|error| KeeperError::Missing(error.to_string()))?;
    super::paths::check_peer(&stream)?;
    stream.set_read_timeout(Some(CLAIM_TIMEOUT))?;
    let mut reader = stream.try_clone()?;
    let mut writer = stream.try_clone()?;

    let hello: wire::Hello =
        expect(&mut reader, Kind::Hello, wire::MAX_HANDSHAKE_FRAME)?.parse()?;
    if !(wire::PROTOCOL_MIN..=wire::PROTOCOL).contains(&hello.protocol) {
        // Understood by every keeper version.
        let _ = wire::write_frame(&mut writer, Kind::Shutdown, b"{}");
        return Err(KeeperError::Version(hello.protocol));
    }
    if hello.session_id != record.session_id {
        return Err(KeeperError::Mismatch("session"));
    }
    if hello.keeper_pid != record.keeper_pid || hello.shell_pid != record.shell_pid {
        return Err(KeeperError::Mismatch("process"));
    }
    // A reused PID never has the recorded start time.
    if super::process_started_at(record.keeper_pid) != Some(record.keeper_started_at) {
        return Err(KeeperError::Mismatch("keeper start time"));
    }
    if record.shell_started_at != 0
        && super::process_started_at(record.shell_pid)
            .is_some_and(|at| at != record.shell_started_at)
    {
        return Err(KeeperError::Mismatch("shell start time"));
    }
    let claim = wire::Claim {
        protocol: wire::PROTOCOL,
        proof: wire::proof(&secret, &hello.nonce),
        resume_from,
    };
    wire::write_frame(&mut writer, Kind::Claim, &serde_json::to_vec(&claim)?)?;
    // A keeper that does not accept the proof closes the connection.
    let claimed: wire::Claimed = expect(&mut reader, Kind::Claimed, wire::MAX_FRAME)?.parse()?;

    let mut checkpoint: Option<(u64, Vec<u8>)> = None;
    let mut building: Vec<u8> = Vec::new();
    let mut gap = None;
    let mut replay = Vec::new();
    loop {
        let frame = wire::read_frame(&mut reader, wire::MAX_FRAME)?
            .ok_or_else(|| KeeperError::Missing("closed during replay".into()))?;
        match frame.kind {
            Kind::Checkpoint => {
                let (at, last, data) = wire::parse_checkpoint_piece(&frame.payload)?;
                if building.len() + data.len() > wire::MAX_CHECKPOINT {
                    return Err(KeeperError::Mismatch("checkpoint too large"));
                }
                building.extend_from_slice(data);
                if last {
                    checkpoint = Some((at, std::mem::take(&mut building)));
                }
            }
            Kind::Gap => gap = Some(frame.parse::<wire::Gap>()?),
            Kind::Replay => replay.extend_from_slice(&frame.payload),
            Kind::ReplayDone => break,
            _ => return Err(KeeperError::Mismatch("unexpected frame during replay")),
        }
    }
    let wanted = checkpoint
        .as_ref()
        .map_or(resume_from, |(at, _)| (*at).min(resume_from));
    let replay_from = gap.map_or(wanted, |gap| gap.to.max(wanted));
    stream.set_read_timeout(None)?;
    Ok(Claimed {
        stream,
        claimed,
        checkpoint,
        gap,
        replay,
        replay_from,
    })
}

fn expect(reader: &mut impl Read, kind: Kind, max: usize) -> Result<wire::Frame, KeeperError> {
    match wire::read_frame(reader, max)? {
        Some(frame) if frame.kind == kind => Ok(frame),
        Some(_) => Err(KeeperError::Mismatch("unexpected frame")),
        None => Err(KeeperError::Missing("closed the connection".into())),
    }
}

/// The child's exit as the keeper reported it; `Lost` when the keeper went
/// without reporting one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Running,
    Exited(i32),
    Lost,
}

/// Present a claimed keeper as a PTY.
/// `first` is output to deliver before the live stream: what this daemon has
/// not processed yet.
pub fn into_spawned(
    session_id: SessionId,
    record: KeeperRecord,
    claimed: &Claimed,
    first: Vec<u8>,
) -> Result<SpawnedPty, KeeperError> {
    let stream = claimed.stream.try_clone()?;
    let writer = Arc::new(Mutex::new(claimed.stream.try_clone()?));
    let status = Arc::new(Mutex::new(match claimed.claimed.exited {
        Some(code) => Status::Exited(code),
        None => Status::Running,
    }));
    let reader = FrameReader {
        stream,
        ack: writer.clone(),
        status: status.clone(),
        current: first,
        position: 0,
    };
    let input = InputWriter {
        writer: writer.clone(),
    };
    let resize_writer = writer.clone();
    let resize = Arc::new(move |cols: u16, rows: u16| {
        send(&resize_writer, Kind::Resize, &wire::Resize { cols, rows })
            .map_err(|error| operation(session_id, "resize", error))
    });
    let kill_writer = writer.clone();
    let kill = Arc::new(move || {
        send(
            &kill_writer,
            Kind::Signal,
            &wire::Signal {
                signal: wire::SignalName::Hup,
            },
        )
        .map_err(|error| operation(session_id, "signal", error))
    });
    let wait_status = status;
    let try_wait = Arc::new(move || match wait_status.lock().map(|status| *status) {
        Ok(Status::Running) => Ok(None),
        Ok(Status::Exited(code)) => Ok(Some(code)),
        Ok(Status::Lost) => Err(PtyError::KeeperLost(session_id)),
        Err(_) => Err(operation(session_id, "poll", "lock poisoned")),
    });
    let checkpoint_writer = writer;
    let checkpoint = Arc::new(move |at: u64, snapshot: &[u8]| {
        if snapshot.len() > wire::MAX_CHECKPOINT {
            return;
        }
        if let Ok(mut out) = checkpoint_writer.lock() {
            let _ = wire::write_checkpoint(&mut *out, at, snapshot);
        }
    });
    Ok(SpawnedPty {
        root_pid: Some(record.shell_pid),
        reader: Box::new(reader),
        writer: Arc::new(Mutex::new(Box::new(input))),
        resize,
        kill,
        try_wait,
        keeper: Some(KeptPty { record, checkpoint }),
    })
}

fn operation(
    session_id: SessionId,
    operation: &'static str,
    error: impl std::fmt::Display,
) -> PtyError {
    PtyError::Operation {
        operation,
        session_id,
        message: error.to_string(),
    }
}

fn send(
    writer: &Mutex<UnixStream>,
    kind: Kind,
    value: &impl serde::Serialize,
) -> std::io::Result<()> {
    let payload = serde_json::to_vec(value)?;
    let mut out = writer
        .lock()
        .map_err(|_| std::io::Error::other("keeper writer lock poisoned"))?;
    wire::write_frame(&mut *out, kind, &payload)
}

/// Live output from the keeper as a byte stream. The exit status ends it.
struct FrameReader {
    stream: UnixStream,
    ack: Arc<Mutex<UnixStream>>,
    status: Arc<Mutex<Status>>,
    current: Vec<u8>,
    position: usize,
}

impl Read for FrameReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        while self.position >= self.current.len() {
            match wire::read_frame(&mut self.stream, wire::MAX_FRAME) {
                Ok(Some(frame)) => match frame.kind {
                    Kind::Output | Kind::Replay => {
                        self.current = frame.payload;
                        self.position = 0;
                    }
                    Kind::Exited => {
                        let exited: wire::Exited = frame.parse()?;
                        if let Ok(mut status) = self.status.lock() {
                            *status = Status::Exited(exited.code);
                        }
                        let _ = send(&self.ack, Kind::Ack, &serde_json::json!({}));
                        return Ok(0);
                    }
                    Kind::Superseded => {
                        return Err(std::io::Error::other(
                            "another daemon claimed this terminal",
                        ));
                    }
                    _ => {}
                },
                Ok(None) | Err(_) => {
                    if let Ok(mut status) = self.status.lock() {
                        if *status == Status::Running {
                            *status = Status::Lost;
                        }
                    }
                    return Ok(0);
                }
            }
        }
        let count = buffer.len().min(self.current.len() - self.position);
        buffer[..count].copy_from_slice(&self.current[self.position..self.position + count]);
        self.position += count;
        Ok(count)
    }
}

/// Terminal input to the keeper, in frames.
struct InputWriter {
    writer: Arc<Mutex<UnixStream>>,
}

impl Write for InputWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let mut out = self
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("keeper writer lock poisoned"))?;
        wire::write_chunked(&mut *out, Kind::Input, bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Start a keeper for a new Session and claim it. `osc_nonce` is the
/// Session's shell hook nonce (in `request.env`), kept for a restart.
pub fn start(
    request: &PtySpawnRequest,
    socket_dir: &Path,
    osc_nonce: &str,
    now_ms: i64,
) -> Result<(SpawnedPty, Claimed), KeeperError> {
    super::paths::ensure_private_dir(socket_dir)?;
    let socket = super::paths::socket_path(socket_dir, request.session_id.0)
        .ok_or(KeeperError::Mismatch("socket path too long"))?;
    let secret = crate::auth::secure_random::<32>().map_err(std::io::Error::other)?;
    let config = super::process::KeeperConfig {
        request: request.clone(),
        socket: socket.clone(),
        secret: wire::hex(&secret),
    };
    let (mut child, scope_unit) = launch(socket_dir, request.session_id.0)?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("no stdin"))?;
        stdin.write_all(&serde_json::to_vec(&config)?)?;
    }
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("no stdout"))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let _ = std::io::BufReader::new(stdout).read_line(&mut line);
        let _ = sender.send(line);
    });
    let line = receiver
        .recv_timeout(READY_TIMEOUT)
        .map_err(|_| KeeperError::Missing("the keeper did not start".into()))?;
    let ready: super::process::Ready = serde_json::from_str(line.trim())
        .map_err(|_| KeeperError::Missing("the keeper did not start".into()))?;
    // The keeper lives on its own; nothing here waits for it.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    let record = KeeperRecord {
        session_id: request.session_id.0,
        socket_path: socket,
        secret_hex: wire::hex(&secret),
        osc_nonce: osc_nonce.to_string(),
        keeper_pid: ready.keeper_pid,
        keeper_started_at: super::process_started_at(ready.keeper_pid).unwrap_or(0),
        shell_pid: ready.shell_pid,
        shell_started_at: super::process_started_at(ready.shell_pid).unwrap_or(0),
        protocol: wire::PROTOCOL,
        scope_unit,
        created_at: now_ms,
    };
    let claimed = claim(&record, 0)?;
    // Everything the shell printed before the claim is new to the daemon.
    let first = claimed.replay.clone();
    let spawned = into_spawned(request.session_id, record, &claimed, first)?;
    Ok((spawned, claimed))
}

/// Start `sentinald keeper` outside the daemon's kill scope (§4): its own
/// systemd scope under systemd, its own process group everywhere.
fn launch(
    socket_dir: &Path,
    session_id: u64,
) -> std::io::Result<(std::process::Child, Option<String>)> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe()?;
    let under_systemd = cfg!(target_os = "linux") && std::env::var_os("INVOCATION_ID").is_some();
    let unit = under_systemd.then(|| {
        let name = socket_dir
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("sentinal-keeper-{name}-{session_id}")
    });
    let mut command = match &unit {
        Some(unit) => {
            let mut command = Command::new("systemd-run");
            command
                .args(["--user", "--scope", "--collect", "--quiet"])
                .arg(format!("--unit={unit}"))
                .arg("--")
                .arg(&exe)
                .arg("keeper");
            command
        }
        None => {
            let mut command = Command::new(&exe);
            command.arg("keeper");
            command
        }
    };
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    match command.spawn() {
        Ok(child) => Ok((child, unit)),
        Err(error) if unit.is_some() => {
            // No systemd-run: survive a crash, if not a `systemctl` restart.
            tracing::warn!(%error, "systemd-run unavailable; starting the keeper directly");
            let mut command = Command::new(&exe);
            command
                .arg("keeper")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .process_group(0);
            Ok((command.spawn()?, None))
        }
        Err(error) => Err(error),
    }
}

/// Ask whatever keeper answers at `socket` to stop (Q3, uninstall).
pub fn shut_down(socket: &PathBuf) {
    if let Ok(mut stream) = UnixStream::connect(socket) {
        let _ = stream.set_read_timeout(Some(CLAIM_TIMEOUT));
        let _ = wire::read_frame(&mut stream, wire::MAX_HANDSHAKE_FRAME);
        let _ = wire::write_frame(&mut stream, Kind::Shutdown, b"{}");
    }
}
