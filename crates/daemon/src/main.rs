//! The terminal-agent daemon executable.

use daemon::{
    api::{router, AppState},
    audit::AuditLog,
    auth::{load_or_create_token, AuthError, AuthManager},
    config::{ConfigError, DaemonConfig},
    pty::{FakePty, PtyBackend, RealPty},
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{Machine, MachineId, MachineStatus, Os, Timestamp};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Debug, thiserror::Error)]
enum MainError {
    #[error("HOME is not set; cannot locate daemon configuration")]
    MissingHome,
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("failed to initialize tracing: {0}")]
    Tracing(String),
    #[error("failed to bind daemon to {address}: {source}")]
    Bind {
        address: SocketAddr,
        source: std::io::Error,
    },
    #[error("daemon server failed: {0}")]
    Serve(std::io::Error),
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tad=info".into()),
        )
        .try_init()
        .map_err(|error| MainError::Tracing(error.to_string()))?;

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(MainError::MissingHome)?;
    let config_path = std::env::var_os("TA_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config/terminal-agent/daemon.toml"));
    let config = DaemonConfig::load(&config_path, &home)?;
    let token = load_or_create_token(&config.token_file)?;
    let auth_file = std::env::var_os("TA_AUTH_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| config.auth_file.clone());
    let auth = AuthManager::load(&auth_file, token)?;
    let machine = machine_description();
    let backend: Arc<dyn PtyBackend> = if std::env::var_os("TA_FAKE_PTY").is_some() {
        tracing::warn!("using fake PTY backend");
        Arc::new(FakePty::new())
    } else {
        Arc::new(RealPty)
    };
    let sessions = SessionManager::new(
        backend,
        SessionManagerConfig::system(
            machine.id.clone(),
            config.data_dir.clone(),
            config.scrollback_lines,
        ),
        AuditLog::new(&config.data_dir),
    );
    let state = AppState::new(sessions, machine, auth);
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|source| MainError::Bind {
            address: config.bind,
            source,
        })?;
    tracing::info!(address = %config.bind, config = %config_path.display(), "daemon listening");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .map_err(MainError::Serve)
}

fn machine_description() -> Machine {
    let name = machine_name();
    Machine {
        id: MachineId(slug(&name)),
        name,
        os: operating_system(),
        cpu: "unknown".into(),
        cpu_cores: std::thread::available_parallelism()
            .map(|cores| u32::try_from(cores.get()).unwrap_or(u32::MAX))
            .unwrap_or(1),
        ram_bytes: 0,
        gpus: vec![],
        status: MachineStatus::Online,
        last_seen: now(),
    }
}

fn machine_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .or_else(|| {
            std::fs::read_to_string(Path::new("/etc/hostname"))
                .ok()
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| "terminal-agent-machine".into())
}

fn slug(name: &str) -> String {
    let slug = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    slug.trim_matches('-').to_string()
}

fn operating_system() -> Os {
    if cfg!(target_os = "macos") {
        Os::Macos
    } else {
        Os::Linux
    }
}

fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    Timestamp(i64::try_from(millis).unwrap_or(i64::MAX))
}
