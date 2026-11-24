//! Keepers (T5, `docs/R1_KEEPER_DESIGN.md`): one small `sentinald keeper`
//! process per FULL Session owns the PTY, the shell and a ring of recent
//! output, so the Session outlives a daemon restart or update. The daemon
//! talks to it over a private Unix socket and keeps everything else.

pub mod paths;
pub mod protocol;
pub mod ring;

/// Output a keeper holds for a daemon that is away (D2, Q7).
pub const RING_BYTES: usize = 1024 * 1024;
/// A keeper with no daemon for this long ends its Session (D3).
pub const ORPHAN_SECS: u64 = 24 * 60 * 60;

pub mod client;
pub mod process;

/// What the daemon stores to reclaim a keeper after a restart
/// (`docs/R1_KEEPER_DESIGN.md` §8, table `session_keepers`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct KeeperRecord {
    /// The Session.
    pub session_id: u64,
    /// The keeper's socket.
    pub socket_path: std::path::PathBuf,
    /// The per-keeper secret, hex. Never logged; never leaves the Machine.
    pub secret_hex: String,
    /// The shell hook nonce, so a restarted daemon accepts the same shell's signals.
    pub osc_nonce: String,
    /// The keeper process.
    pub keeper_pid: u32,
    /// Its start time, seconds since the epoch (a reused PID never matches).
    pub keeper_started_at: u64,
    /// The shell.
    pub shell_pid: u32,
    /// Its start time.
    pub shell_started_at: u64,
    /// The protocol it speaks.
    pub protocol: u32,
    /// The systemd scope it runs in (Linux), if any.
    pub scope_unit: Option<String>,
    /// When it was recorded, Unix milliseconds.
    pub created_at: i64,
}

/// Start time of `pid` in seconds since the epoch, if it is running.
pub fn process_started_at(pid: u32) -> Option<u64> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::nothing(),
    );
    system.process(pid).map(sysinfo::Process::start_time)
}

/// PTYs held by keepers, so Sessions survive a daemon restart (T5). A
/// Session whose keeper cannot start gets the daemon's own PTY instead and
/// says it will not survive one.
#[derive(Debug)]
pub struct KeeperPty {
    socket_dir: std::path::PathBuf,
    clock: fn() -> i64,
}

impl KeeperPty {
    /// Keepers with sockets under `socket_dir`.
    pub fn new(socket_dir: std::path::PathBuf, clock: fn() -> i64) -> Self {
        Self { socket_dir, clock }
    }

    /// Where this daemon's keeper sockets live.
    pub fn socket_dir(&self) -> &std::path::Path {
        &self.socket_dir
    }
}

impl crate::pty::PtyBackend for KeeperPty {
    fn login_env(&self, shell: &str) -> Option<Vec<(String, String)>> {
        crate::pty::RealPty.login_env(shell)
    }

    fn spawn(
        &self,
        request: &crate::pty::PtySpawnRequest,
    ) -> Result<crate::pty::SpawnedPty, crate::pty::PtyError> {
        let nonce = request
            .env
            .iter()
            .find(|(name, _)| name == crate::osc::NONCE_ENV)
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        match client::start(request, &self.socket_dir, &nonce, (self.clock)()) {
            Ok((spawned, _)) => Ok(spawned),
            Err(error) => {
                tracing::warn!(
                    session = request.session_id.0,
                    %error,
                    "no keeper for this terminal; it ends if Sentinal restarts"
                );
                crate::pty::PtyBackend::spawn(&crate::pty::RealPty, request)
            }
        }
    }
}

/// The store's row for a keeper.
pub fn to_stored(record: &KeeperRecord) -> store::StoredKeeper {
    store::StoredKeeper {
        session_id: record.session_id,
        socket_path: record.socket_path.to_string_lossy().into_owned(),
        secret_hex: record.secret_hex.clone(),
        osc_nonce: record.osc_nonce.clone(),
        keeper_pid: record.keeper_pid,
        keeper_started_at: record.keeper_started_at,
        shell_pid: record.shell_pid,
        shell_started_at: record.shell_started_at,
        protocol: record.protocol,
        scope_unit: record.scope_unit.clone(),
        created_at: record.created_at,
    }
}

/// A keeper from the store's row.
pub fn from_stored(stored: &store::StoredKeeper) -> KeeperRecord {
    KeeperRecord {
        session_id: stored.session_id,
        socket_path: stored.socket_path.clone().into(),
        secret_hex: stored.secret_hex.clone(),
        osc_nonce: stored.osc_nonce.clone(),
        keeper_pid: stored.keeper_pid,
        keeper_started_at: stored.keeper_started_at,
        shell_pid: stored.shell_pid,
        shell_started_at: stored.shell_started_at,
        protocol: stored.protocol,
        scope_unit: stored.scope_unit.clone(),
        created_at: stored.created_at,
    }
}

/// `sentinald keeper stop-all`: end every keeper of the daemon configured by
/// `TA_CONFIG` (or the default config), for uninstalling and for a rollback
/// to a release from before keepers (Q3). Their shells are hung up. Returns
/// how many keepers were asked.
pub fn stop_all() -> std::io::Result<usize> {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .ok_or_else(|| std::io::Error::other("HOME is not set"))?;
    let config_path = std::env::var_os("TA_CONFIG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| home.join(".config/terminal-agent/daemon.toml"));
    let config = crate::config::DaemonConfig::load(&config_path, &home)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let dir = paths::socket_dir(&config.data_dir);
    let mut asked = 0;
    for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "sock")
        {
            client::shut_down(&path);
            asked += 1;
        }
    }
    Ok(asked)
}
