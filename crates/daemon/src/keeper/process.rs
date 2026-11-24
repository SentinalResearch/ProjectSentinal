//! `sentinald keeper`: the process that holds one Session's terminal
//! (`docs/R1_KEEPER_DESIGN.md` §4–5, §9).
//!
//! It reads its configuration (with the secret) from stdin, opens the PTY
//! and starts the shell, binds its `0600` socket, prints one ready line on
//! stdout, and then runs on plain threads: the PTY reader fills the ring; a
//! waiter records the child's exit; the listener takes one authenticated
//! controller at a time and streams output to it. No async runtime, no
//! network socket, no configuration file.

use super::protocol::{self as wire, Kind};
use super::ring::Ring;
use crate::pty::PtySpawnRequest;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Largest configuration read from stdin.
const MAX_CONFIG: u64 = 1024 * 1024;
/// How long a connection may take to authenticate.
const HANDSHAKE: Duration = Duration::from_secs(2);
/// A `HUP` that does not end the child is followed by `KILL` after this.
const KILL_AFTER: Duration = Duration::from_secs(5);

/// What the daemon hands a new keeper on stdin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeeperConfig {
    /// The shell to start.
    pub request: PtySpawnRequest,
    /// Where to listen.
    pub socket: PathBuf,
    /// The secret, hex.
    pub secret: String,
}

/// The keeper's one line on stdout once it listens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ready {
    /// The keeper's PID.
    pub keeper_pid: u32,
    /// The shell's PID.
    pub shell_pid: u32,
}

type Writer = Arc<Mutex<UnixStream>>;

struct State {
    ring: Ring,
    /// Offset the current controller has been sent up to.
    sent: u64,
    /// The current controller: its generation and its write side.
    controller: Option<(u64, Writer)>,
    generation: u64,
    exit: Option<i32>,
    exit_sent: bool,
    acked: bool,
    pty_closed: bool,
    shutdown: bool,
    size: (u16, u16),
    checkpoint: Option<(u64, Vec<u8>)>,
    building: Option<(u64, Vec<u8>)>,
    last_controller: Instant,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    secret: Vec<u8>,
    session_id: u64,
    keeper_pid: u32,
    shell_pid: u32,
    master: Mutex<Box<dyn portable_pty::MasterPty + Send>>,
    input: Mutex<Box<dyn Write + Send>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Run as a keeper. Returns the process exit code.
pub fn main() -> i32 {
    match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("sentinald keeper: {error}");
            1
        }
    }
}

fn run() -> std::io::Result<()> {
    let mut text = String::new();
    std::io::stdin()
        .take(MAX_CONFIG)
        .read_to_string(&mut text)?;
    let config: KeeperConfig = serde_json::from_str(&text)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let secret = wire::unhex(&config.secret)
        .filter(|secret| secret.len() >= 16)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "bad secret"))?;
    if let Some(dir) = config.socket.parent() {
        super::paths::check_private(dir, 0o700)?;
    }
    let _ = std::fs::remove_file(&config.socket);
    let listener = UnixListener::bind(&config.socket)?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config.socket, std::fs::Permissions::from_mode(0o600))?;
    }

    let shell = crate::pty::open_shell(&config.request)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let shell_pid = shell.child.process_id().unwrap_or(0);
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            ring: Ring::new(super::RING_BYTES),
            sent: 0,
            controller: None,
            generation: 0,
            exit: None,
            exit_sent: false,
            acked: false,
            pty_closed: false,
            shutdown: false,
            size: (config.request.cols, config.request.rows),
            checkpoint: None,
            building: None,
            last_controller: Instant::now(),
        }),
        changed: Condvar::new(),
        secret,
        session_id: config.request.session_id.0,
        keeper_pid: std::process::id(),
        shell_pid,
        master: Mutex::new(shell.master),
        input: Mutex::new(shell.writer),
    });

    spawn_reader(shared.clone(), shell.reader);
    spawn_waiter(shared.clone(), shell.child);
    spawn_listener(shared.clone(), listener);

    // The daemon waits for this line; then stdout is closed.
    let ready = Ready {
        keeper_pid: shared.keeper_pid,
        shell_pid,
    };
    {
        let mut stdout = std::io::stdout().lock();
        writeln!(stdout, "{}", serde_json::to_string(&ready)?)?;
        stdout.flush()?;
    }

    // Wait until the exit has been handed over, a daemon asked us to stop,
    // or no daemon came back for `ORPHAN_SECS`.
    let orphan = Duration::from_secs(super::ORPHAN_SECS);
    let mut state = shared.lock();
    loop {
        if state.shutdown || (state.exit.is_some() && state.acked) {
            break;
        }
        if state.controller.is_none() && state.last_controller.elapsed() >= orphan {
            state.shutdown = true;
            break;
        }
        state = shared
            .changed
            .wait_timeout(state, Duration::from_secs(60))
            .map(|(state, _)| state)
            .unwrap_or_else(|error| error.into_inner().0);
    }
    let child_alive = state.exit.is_none();
    drop(state);
    if child_alive {
        signal_child(&shared, wire::SignalName::Hup);
    }
    let _ = std::fs::remove_file(&config.socket);
    Ok(())
}

/// PTY → ring. While a daemon is connected, never overwrite what it has not
/// been sent (the shell waits instead); without one, keep the newest bytes.
fn spawn_reader(shared: Arc<Shared>, mut reader: Box<dyn Read + Send>) {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(count) => count,
            };
            let mut state = shared.lock();
            while state.controller.is_some()
                && state.ring.end() - state.sent + count as u64 > super::RING_BYTES as u64
            {
                state = shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            state.ring.push(&buffer[..count]);
            drop(state);
            shared.changed.notify_all();
        }
        shared.lock().pty_closed = true;
        shared.changed.notify_all();
    });
}

fn spawn_waiter(shared: Arc<Shared>, mut child: Box<dyn portable_pty::Child + Send + Sync>) {
    std::thread::spawn(move || {
        let code = child
            .wait()
            .map(|status| i32::try_from(status.exit_code()).unwrap_or(-1))
            .unwrap_or(-1);
        // The child's last output may still be in the PTY: let the reader
        // take it before the exit is reported after it.
        std::thread::sleep(Duration::from_millis(200));
        shared.lock().exit = Some(code);
        shared.changed.notify_all();
    });
}

fn spawn_listener(shared: Arc<Shared>, listener: UnixListener) {
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let shared = shared.clone();
            std::thread::spawn(move || {
                let _ = serve(&shared, stream);
            });
        }
    });
}

/// One connection: authenticate it, make it the controller, replay, then
/// stream output until it goes or another daemon takes over.
fn serve(shared: &Arc<Shared>, stream: UnixStream) -> std::io::Result<()> {
    super::paths::check_peer(&stream)?;
    stream.set_read_timeout(Some(HANDSHAKE))?;
    let mut reader = stream.try_clone()?;
    let writer: Writer = Arc::new(Mutex::new(stream.try_clone()?));
    let nonce = wire::hex(&crate::auth::secure_random::<16>().map_err(std::io::Error::other)?);
    let hello = wire::Hello {
        protocol: wire::PROTOCOL,
        keeper_version: crate::DAEMON_VERSION.into(),
        session_id: shared.session_id,
        keeper_pid: shared.keeper_pid,
        shell_pid: shared.shell_pid,
        nonce: nonce.clone(),
    };
    send(&writer, Kind::Hello, &serde_json::to_vec(&hello)?)?;
    let Some(first) = wire::read_frame(&mut reader, wire::MAX_HANDSHAKE_FRAME)? else {
        return Ok(());
    };
    if first.kind == Kind::Shutdown {
        // Understood by every version, from any process of this user.
        shared.lock().shutdown = true;
        shared.changed.notify_all();
        return Ok(());
    }
    if first.kind != Kind::Claim {
        return Ok(());
    }
    let claim: wire::Claim = first.parse()?;
    if !(wire::PROTOCOL_MIN..=wire::PROTOCOL).contains(&claim.protocol)
        || !wire::proof_matches(&shared.secret, &nonce, &claim.proof)
    {
        return Ok(());
    }
    stream.set_read_timeout(None)?;

    // Take over from any earlier controller.
    let (generation, claimed, checkpoint, replay_from, replay_to) = {
        let mut state = shared.lock();
        state.generation += 1;
        let generation = state.generation;
        if let Some((_, old)) = state.controller.take() {
            let _ = send(&old, Kind::Superseded, b"{}");
            if let Ok(old) = old.lock() {
                let _ = old.shutdown(std::net::Shutdown::Both);
            }
        }
        state.controller = Some((generation, writer.clone()));
        state.last_controller = Instant::now();
        state.exit_sent = false;
        let checkpoint = state.checkpoint.clone();
        let replay_from = checkpoint
            .as_ref()
            .map_or(claim.resume_from, |(at, _)| (*at).min(claim.resume_from));
        let claimed = wire::Claimed {
            output_end: state.ring.end(),
            ring_start: state.ring.start(),
            checkpoint_at: checkpoint.as_ref().map(|(at, _)| *at),
            size: state.size,
            exited: state.exit,
        };
        let replay_to = state.ring.end();
        state.sent = replay_to;
        (generation, claimed, checkpoint, replay_from, replay_to)
    };
    send(&writer, Kind::Claimed, &serde_json::to_vec(&claimed)?)?;
    if let Some((at, snapshot)) = checkpoint {
        let mut out = lock(&writer)?;
        wire::write_checkpoint(&mut *out, at, &snapshot)?;
    }
    let read = shared.lock().ring.read(
        replay_from,
        (replay_to.saturating_sub(replay_from)) as usize,
    );
    if let Some((from, to)) = read.gap {
        send(
            &writer,
            Kind::Gap,
            &serde_json::to_vec(&wire::Gap { from, to })?,
        )?;
    }
    {
        let mut out = lock(&writer)?;
        wire::write_chunked(&mut *out, Kind::Replay, &read.bytes)?;
        wire::write_frame(&mut *out, Kind::ReplayDone, b"{}")?;
    }
    shared.changed.notify_all();

    let receiver = {
        let shared = shared.clone();
        std::thread::spawn(move || receive(&shared, reader, generation))
    };
    let result = stream_output(shared, &writer, generation);
    let _ = receiver.join();
    result
}

fn lock(writer: &Writer) -> std::io::Result<MutexGuard<'_, UnixStream>> {
    writer
        .lock()
        .map_err(|_| std::io::Error::other("writer lock poisoned"))
}

fn send(writer: &Writer, kind: Kind, payload: &[u8]) -> std::io::Result<()> {
    let mut out = lock(writer)?;
    wire::write_frame(&mut *out, kind, payload)
}

/// Ring → controller, then the exit status once all output is sent.
fn stream_output(shared: &Arc<Shared>, writer: &Writer, generation: u64) -> std::io::Result<()> {
    loop {
        let (bytes, exited) = {
            let mut state = shared.lock();
            loop {
                if state.controller.as_ref().map(|(g, _)| *g) != Some(generation) {
                    return Ok(());
                }
                let unsent = state.sent < state.ring.end();
                let exit_due = state.exit.is_some() && !state.exit_sent && !unsent;
                if unsent || exit_due {
                    break;
                }
                state = shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
            let read = state.ring.read(state.sent, wire::MAX_FRAME);
            state.sent += read.bytes.len() as u64;
            let exited = match state.exit {
                Some(code) if !state.exit_sent && state.sent >= state.ring.end() => {
                    state.exit_sent = true;
                    Some(code)
                }
                _ => None,
            };
            (read.bytes, exited)
        };
        shared.changed.notify_all();
        if !bytes.is_empty() {
            send(writer, Kind::Output, &bytes)?;
        }
        if let Some(code) = exited {
            send(
                writer,
                Kind::Exited,
                &serde_json::to_vec(&wire::Exited { code })?,
            )?;
        }
    }
}

/// Controller → keeper: input, resize, signals, checkpoints, the ack.
fn receive(shared: &Arc<Shared>, mut reader: UnixStream, generation: u64) {
    while let Ok(Some(frame)) = wire::read_frame(&mut reader, wire::MAX_FRAME) {
        let current = shared.lock().controller.as_ref().map(|(g, _)| *g) == Some(generation);
        if !current {
            break;
        }
        match frame.kind {
            Kind::Input => {
                if let Ok(mut input) = shared.input.lock() {
                    let _ = input.write_all(&frame.payload).and_then(|()| input.flush());
                }
            }
            Kind::Resize => {
                if let Ok(size) = frame.parse::<wire::Resize>() {
                    if let Ok(master) = shared.master.lock() {
                        let _ = master.resize(portable_pty::PtySize {
                            rows: size.rows,
                            cols: size.cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        });
                    }
                    shared.lock().size = (size.cols, size.rows);
                }
            }
            Kind::Signal => {
                if let Ok(signal) = frame.parse::<wire::Signal>() {
                    signal_child(shared, signal.signal);
                    if signal.signal == wire::SignalName::Hup {
                        let shared = shared.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(KILL_AFTER);
                            if shared.lock().exit.is_none() {
                                signal_child(&shared, wire::SignalName::Kill);
                            }
                        });
                    }
                }
            }
            Kind::Checkpoint => keep_checkpoint(shared, &frame.payload),
            Kind::Ack => {
                shared.lock().acked = true;
                shared.changed.notify_all();
            }
            Kind::Shutdown => {
                shared.lock().shutdown = true;
                shared.changed.notify_all();
            }
            _ => {}
        }
    }
    let mut state = shared.lock();
    if state.controller.as_ref().map(|(g, _)| *g) == Some(generation) {
        state.controller = None;
        state.last_controller = Instant::now();
    }
    drop(state);
    shared.changed.notify_all();
}

/// Assemble checkpoint pieces; a whole one replaces the previous.
fn keep_checkpoint(shared: &Arc<Shared>, payload: &[u8]) {
    let Ok((at, last, data)) = wire::parse_checkpoint_piece(payload) else {
        return;
    };
    let mut state = shared.lock();
    let building = match state.building.take() {
        Some((building_at, bytes)) if building_at == at => Some((at, bytes)),
        _ => Some((at, Vec::new())),
    };
    let Some((at, mut bytes)) = building else {
        return;
    };
    if bytes.len() + data.len() > wire::MAX_CHECKPOINT {
        return;
    }
    bytes.extend_from_slice(data);
    if last {
        state.checkpoint = Some((at, bytes));
    } else {
        state.building = Some((at, bytes));
    }
}

/// Signal the child's process group; nothing else can be named.
fn signal_child(shared: &Shared, name: wire::SignalName) {
    use nix::sys::signal::{killpg, Signal};
    if shared.shell_pid == 0 || shared.lock().exit.is_some() {
        return;
    }
    let signal = match name {
        wire::SignalName::Hup => Signal::SIGHUP,
        wire::SignalName::Int => Signal::SIGINT,
        wire::SignalName::Term => Signal::SIGTERM,
        wire::SignalName::Kill => Signal::SIGKILL,
    };
    let Ok(pid) = i32::try_from(shared.shell_pid) else {
        return;
    };
    let _ = killpg(nix::unistd::Pid::from_raw(pid), signal);
}
