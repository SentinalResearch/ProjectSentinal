//! Shared helpers for ignored real-shell hook tests.

#![allow(clippy::unwrap_used, clippy::expect_used, dead_code)]

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use protocol::{
    Command, CommandId, Envelope, LocalRequest, LocalResponse, MachineId, Session, SessionId,
    SessionOrigin, Timestamp,
};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
};

/// Prompt text every test shell prints, used to detect a finished Command.
pub const PROMPT: &str = "SNTL-PROMPT> ";
/// Printed once per finished Command by fish, which redraws its prompt
/// several times per line, so prompts cannot be counted there.
const DONE: &str = "SNTL-DONE";

static NEXT: AtomicU64 = AtomicU64::new(1);

/// A short private directory (macOS socket paths must stay under 104 bytes).
pub fn short_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{tag}{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Path of a shell executable on this host, if installed.
pub fn find_shell(name: &str) -> Option<PathBuf> {
    ["/bin", "/usr/bin", "/usr/local/bin", "/opt/homebrew/bin"]
        .iter()
        .map(|dir| Path::new(dir).join(name))
        .find(|path| path.exists())
}

/// `<shell> --version`, first line.
pub fn shell_version(shell: &Path) -> String {
    std::process::Command::new(shell)
        .arg("--version")
        .output()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or_default()
                .to_string()
        })
        .unwrap_or_default()
}

/// Reply behavior of the fake local socket.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Answer like sentinald.
    Answer,
    /// Accept and never reply.
    Silent,
    /// Reply with bytes that are not a frame of JSON.
    Garbage,
}

/// A fake sentinald local socket that records every request.
pub struct FakeSocket {
    /// Socket path.
    pub path: PathBuf,
    /// Connections accepted.
    pub connections: Arc<AtomicUsize>,
    /// Requests received, in arrival order.
    pub requests: Arc<Mutex<Vec<LocalRequest>>>,
    /// Raw request bytes, for privacy assertions.
    pub raw: Arc<Mutex<Vec<u8>>>,
}

impl FakeSocket {
    /// Start listening; must be called inside a Tokio runtime.
    pub fn start(mode: Mode) -> Self {
        let path = short_dir("sks").join("s.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let connections = Arc::new(AtomicUsize::new(0));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let raw = Arc::new(Mutex::new(Vec::new()));
        let (count, log, bytes) = (connections.clone(), requests.clone(), raw.clone());
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                let (log, bytes) = (log.clone(), bytes.clone());
                tokio::spawn(async move {
                    let mut header = [0_u8; 4];
                    if stream.read_exact(&mut header).await.is_err() {
                        return;
                    }
                    let mut body = vec![0; u32::from_be_bytes(header) as usize];
                    if stream.read_exact(&mut body).await.is_err() {
                        return;
                    }
                    bytes.lock().unwrap().extend_from_slice(&body);
                    let Ok(envelope) =
                        Envelope::<LocalRequest>::decode(std::str::from_utf8(&body).unwrap())
                    else {
                        return;
                    };
                    log.lock().unwrap().push(envelope.payload.clone());
                    let reply = match mode {
                        Mode::Silent => {
                            tokio::time::sleep(Duration::from_secs(60)).await;
                            return;
                        }
                        Mode::Garbage => b"{nope".to_vec(),
                        Mode::Answer => Envelope::new(answer(&envelope.payload))
                            .encode()
                            .unwrap()
                            .into_bytes(),
                    };
                    let length = u32::try_from(reply.len()).unwrap();
                    let _ = stream.write_all(&length.to_be_bytes()).await;
                    let _ = stream.write_all(&reply).await;
                });
            }
        });
        FakeSocket {
            path,
            connections,
            requests,
            raw,
        }
    }

    /// Snapshot of requests so far.
    pub fn requests(&self) -> Vec<LocalRequest> {
        self.requests.lock().unwrap().clone()
    }
}

fn answer(request: &LocalRequest) -> LocalResponse {
    match request {
        LocalRequest::RegisterShell {
            pid, shell, cwd, ..
        } => {
            let mut session = Session::new(
                SessionId(1),
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
                SessionId(1),
                MachineId("fake".into()),
                "x",
                "/",
                Timestamp(1),
            ),
        },
    }
}

/// An interactive shell running in a real pseudo-terminal.
pub struct ShellTerm {
    pub name: String,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    output: mpsc::Receiver<Vec<u8>>,
    /// Everything the terminal printed.
    pub seen: String,
    prompts: usize,
}

impl ShellTerm {
    /// Start `name` interactively with the Sentinal hook from `sentinal` loaded.
    pub fn spawn(name: &str, sentinal: &Path, env: &[(&str, String)]) -> Self {
        let shell = find_shell(name).unwrap();
        let home = short_dir("shh");
        let sentinal = sentinal.display().to_string();
        let mut command = CommandBuilder::new(&shell);
        match name {
            "zsh" => {
                std::fs::write(
                    home.join(".zshrc"),
                    format!("PROMPT='{PROMPT}'\nRPROMPT=\neval \"$('{sentinal}' hook zsh)\"\n"),
                )
                .unwrap();
                command.arg("-i");
                command.env("ZDOTDIR", home.display().to_string());
            }
            "bash" => {
                let rc = home.join("bashrc");
                std::fs::write(
                    &rc,
                    format!(
                        "PS1='{PROMPT}'\nPROMPT_COMMAND=\neval \"$('{sentinal}' hook bash)\"\n"
                    ),
                )
                .unwrap();
                command.args(["--noprofile", "--rcfile"]);
                command.arg(rc.display().to_string());
                command.arg("-i");
            }
            "fish" => {
                command.args(["--no-config", "-i", "-C"]);
                command.arg(format!(
                    "set -g fish_greeting; function fish_prompt; printf '%s' '{PROMPT}'; end; function __sntl_done --on-event fish_postexec; printf '{DONE}\\n'; end; '{sentinal}' hook fish | source"
                ));
            }
            other => panic!("unsupported shell {other}"),
        }
        command.env("HOME", home.display().to_string());
        command.env("TERM", "dumb");
        command.env_remove("SENTINAL_SESSION_ID");
        command.env_remove("SENTINAL_DISABLE");
        command.cwd(&home);
        for (key, value) in env {
            command.env(key, value);
        }
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 200,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
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
        let mut term = ShellTerm {
            name: name.into(),
            master: pair.master,
            child,
            writer,
            output,
            seen: String::new(),
            prompts: 0,
        };
        term.wait_prompt(1);
        term
    }

    /// Start `name` (zsh or bash) interactively with `rc` as its rc file in
    /// `home`; returns the terminal and the time until the first prompt.
    pub fn spawn_with_rc(
        name: &str,
        home: &Path,
        rc: &str,
        env: &[(&str, String)],
    ) -> (Self, Duration) {
        let shell = find_shell(name).unwrap();
        let mut command = CommandBuilder::new(&shell);
        match name {
            "zsh" => {
                std::fs::write(home.join(".zshrc"), rc).unwrap();
                command.arg("-i");
                command.env("ZDOTDIR", home.display().to_string());
            }
            "bash" => {
                std::fs::write(home.join(".bashrc"), rc).unwrap();
                command.args(["--noprofile", "--rcfile"]);
                command.arg(home.join(".bashrc").display().to_string());
                command.arg("-i");
            }
            other => panic!("unsupported shell {other}"),
        }
        command.env("HOME", home.display().to_string());
        command.env("TERM", "xterm-256color");
        command.env_remove("SENTINAL_SESSION_ID");
        command.env_remove("SENTINAL_DISABLE");
        command.env_remove("SENTINAL_AUTO_GUARD");
        command.cwd(home);
        for (key, value) in env {
            command.env(key, value);
        }
        let started = Instant::now();
        let mut term = Self::start(name, command);
        term.wait_prompt(1);
        (term, started.elapsed())
    }

    fn start(name: &str, command: CommandBuilder) -> Self {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 200,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
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
        ShellTerm {
            name: name.into(),
            master: pair.master,
            child,
            writer,
            output,
            seen: String::new(),
            prompts: 0,
        }
    }

    /// Type a line without waiting for a prompt (e.g. `exit`), then return the
    /// shell's exit code once it has exited.
    pub fn finish(mut self, line: &str) -> u32 {
        self.writer.write_all(line.as_bytes()).unwrap();
        self.writer.write_all(b"\r").unwrap();
        self.writer.flush().unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                drop(self.master);
                return status.exit_code();
            }
            while let Ok(bytes) = self.output.try_recv() {
                self.seen.push_str(&String::from_utf8_lossy(&bytes));
            }
            if Instant::now() > deadline {
                let _ = self.child.kill();
                panic!(
                    "{}: shell did not exit after {line:?}; saw:\n{}",
                    self.name, self.seen
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The shell's PID.
    pub fn pid(&self) -> u32 {
        self.child.process_id().unwrap()
    }

    fn wait_prompt(&mut self, count: usize) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while self.seen.matches(PROMPT).count() < count {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(remaining) {
                Ok(bytes) => self.seen.push_str(&String::from_utf8_lossy(&bytes)),
                Err(_) => panic!("{}: no prompt #{count}; saw:\n{}", self.name, self.seen),
            }
        }
        self.prompts = count;
    }

    /// Type `line`, press Enter, and wait for the next prompt; returns the
    /// output printed in between and the wall time until the prompt appeared.
    pub fn run(&mut self, line: &str) -> (String, Duration) {
        let before = self.seen.len();
        let started = Instant::now();
        self.writer.write_all(line.as_bytes()).unwrap();
        self.writer.write_all(b"\r").unwrap();
        self.writer.flush().unwrap();
        if self.name == "fish" {
            self.wait_done(before);
        } else {
            self.wait_prompt(self.prompts + 1);
        }
        (self.seen[before..].to_string(), started.elapsed())
    }

    /// Wait for fish's per-Command marker and the prompt drawn after it.
    fn wait_done(&mut self, from: usize) {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let after_done = self.seen[from..]
                .find(&format!("{DONE}\r\n"))
                .or_else(|| self.seen[from..].find(&format!("{DONE}\n")))
                .map(|index| from + index + DONE.len());
            if after_done.is_some_and(|index| self.seen[index..].contains(PROMPT)) {
                return;
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.output.recv_timeout(remaining) {
                Ok(bytes) => self.seen.push_str(&String::from_utf8_lossy(&bytes)),
                Err(_) => panic!("fish: no finished Command; saw:\n{}", self.seen),
            }
        }
    }

    /// Exit the shell and reap it.
    pub fn exit(mut self) {
        let _ = self.writer.write_all(b"exit\r");
        let _ = self.writer.flush();
        let _ = self.child.wait();
        drop(self.master);
    }
}

/// Poll `ready` for up to 10 s (real shells report asynchronously).
pub fn wait_until(what: &str, mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !ready() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A real `sentinald` (RealPty) in a private temp directory.
pub struct Daemon {
    child: std::process::Child,
    /// WebSocket URL.
    pub url: String,
    /// Bearer token file.
    pub token: PathBuf,
    /// Local socket path.
    pub socket: PathBuf,
    /// Data directory (SQLite, audit, logs).
    pub data_dir: PathBuf,
    /// Daemon stdout+stderr log.
    pub log: PathBuf,
    /// The daemon's HOME; shells it starts read their rc files from here.
    pub home: PathBuf,
    root: PathBuf,
}

impl Daemon {
    /// Start the `sentinald` next to `sentinal` and wait until it listens.
    pub fn start(sentinal: &Path) -> Self {
        let sentinald = sentinal.with_file_name("sentinald");
        assert!(
            sentinald.exists(),
            "build the daemon first: cargo build -p daemon --bin sentinald"
        );
        let root = short_dir("sd");
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let data_dir = root.join("data");
        let socket = root.join("run/sentinald.sock");
        let token = root.join("token");
        let config = root.join("daemon.toml");
        std::fs::write(
            &config,
            format!(
                "bind = \"127.0.0.1:{port}\"\ndata_dir = \"{}\"\ntoken_file = \"{}\"\nauth_file = \"{}\"\nlocal_socket = \"{}\"\n",
                data_dir.display(),
                token.display(),
                root.join("auth.json").display(),
                socket.display()
            ),
        )
        .unwrap();
        let log = root.join("daemon.log");
        let log_file = std::fs::File::create(&log).unwrap();
        let child = std::process::Command::new(sentinald)
            .env("TA_CONFIG", &config)
            .env("HOME", &root)
            .env_remove("SENTINAL_SESSION_ID")
            .stdout(log_file.try_clone().unwrap())
            .stderr(log_file)
            .spawn()
            .unwrap();
        wait_until("sentinald to listen", || {
            socket.exists() && std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
        });
        Daemon {
            child,
            url: format!("ws://127.0.0.1:{port}/api/v1/ws"),
            token,
            socket,
            data_dir,
            log,
            home: root.clone(),
            root,
        }
    }

    /// Stop with SIGTERM and remove all files.
    pub fn stop(mut self) {
        let _ = std::process::Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
