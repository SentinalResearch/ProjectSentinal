//! The terminal-agent daemon executable.

use daemon::{
    api::{router, AppState},
    audit::AuditLog,
    auth::{load_or_create_token, AuthError, AuthManager},
    config::{ConfigError, DaemonConfig},
    local_socket::{LocalSocketError, SecurityAudit, UnixSocketServer},
    metrics::{MetricsSampler, ResourceDebouncer},
    pty::{FakePty, PtyBackend, RealPty},
    session_manager::{SessionManager, SessionManagerConfig, SessionManagerError},
};
use protocol::{Machine, MachineId, MachineStatus, Os, Resource, Timestamp};
use std::{
    collections::HashMap,
    fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use store::{Store, StoreError};

#[derive(Debug, thiserror::Error)]
enum MainError {
    #[error("HOME is not set; cannot locate daemon configuration")]
    MissingHome,
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Session(#[from] SessionManagerError),
    #[error("failed to create daemon data directory '{}': {source}", path.display())]
    DataDirectory {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to initialize tracing: {0}")]
    Tracing(String),
    #[error("failed to bind daemon to {address}: {source}")]
    Bind {
        address: SocketAddr,
        source: std::io::Error,
    },
    #[error("daemon server failed: {0}")]
    Serve(std::io::Error),
    #[error(transparent)]
    LocalSocket(#[from] LocalSocketError),
    #[error("local socket task failed: {0}")]
    LocalTask(tokio::task::JoinError),
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
    fs::create_dir_all(&config.data_dir).map_err(|source| MainError::DataDirectory {
        path: config.data_dir.clone(),
        source,
    })?;
    let store = Arc::new(Store::open(
        &config.data_dir.join("terminal-agent.sqlite3"),
    )?);
    store.upsert_machine(&machine)?;
    let interrupted = store.interrupt_orphaned_sessions(now())?;
    if !interrupted.is_empty() {
        tracing::info!(
            count = interrupted.len(),
            "marked restart-orphaned sessions interrupted"
        );
    }
    let backend: Arc<dyn PtyBackend> = if std::env::var_os("TA_FAKE_PTY").is_some() {
        tracing::warn!("using fake PTY backend");
        Arc::new(FakePty::new())
    } else {
        Arc::new(RealPty)
    };
    let sessions = SessionManager::with_store(
        backend,
        SessionManagerConfig::system(
            machine.id.clone(),
            config.data_dir.clone(),
            config.scrollback_lines,
        ),
        AuditLog::new(&config.data_dir),
        store.clone(),
    )?;
    spawn_metric_sampler(sessions.clone(), store);
    let state = AppState::new(sessions, machine, auth);
    let local_socket =
        UnixSocketServer::bind(&config.local_socket, SecurityAudit::new(&config.data_dir))?;
    let (local_shutdown, local_shutdown_rx) = tokio::sync::watch::channel(false);
    let local_task = tokio::spawn(local_socket.serve(local_shutdown_rx));
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|source| MainError::Bind {
            address: config.bind,
            source,
        })?;
    tracing::info!(address = %config.bind, config = %config_path.display(), "daemon listening");
    let server_result = axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .map_err(MainError::Serve);
    let _ = local_shutdown.send(true);
    local_task.await.map_err(MainError::LocalTask)??;
    server_result
}

async fn shutdown_signal() {
    let control_c = tokio::signal::ctrl_c();
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                let _ = signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };
    tokio::select! {
        result = control_c => {
            if let Err(error) = result {
                tracing::error!(%error, "failed to wait for Ctrl-C");
            }
        }
        () = terminate => {}
    }
}

fn spawn_metric_sampler(sessions: SessionManager, store: Arc<Store>) {
    tokio::spawn(async move {
        let mut sampler = MetricsSampler::new();
        let mut debouncers = HashMap::<protocol::SessionId, ResourceDebouncer>::new();
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let roots = match sessions.process_roots() {
                Ok(roots) => roots,
                Err(error) => {
                    tracing::warn!(%error, "failed to list Session process roots");
                    continue;
                }
            };
            let at = now();
            let snapshot = sampler.sample(at, &roots);
            let ram_total = snapshot.machine.ram_total_bytes;
            let gpu_percent = snapshot
                .machine
                .gpus
                .iter()
                .map(|gpu| gpu.util_percent)
                .reduce(f32::max);
            let vram_total = snapshot.machine.gpus.iter().fold(0_u64, |total, gpu| {
                total.saturating_add(gpu.vram_total_bytes)
            });
            if let Err(error) = sessions.publish_machine_metrics(snapshot.machine) {
                tracing::warn!(%error, "failed to publish Machine metrics");
            }
            for metrics in snapshot.sessions {
                let mut values = vec![(Resource::Cpu, metrics.totals.cpu_percent)];
                if ram_total > 0 {
                    values.push((
                        Resource::Ram,
                        (metrics.totals.rss_bytes as f64 / ram_total as f64 * 100.0) as f32,
                    ));
                }
                if metrics.totals.gpu_mem_bytes > 0 {
                    if let Some(percent) = gpu_percent {
                        values.push((Resource::Gpu, percent));
                    }
                    if vram_total > 0 {
                        values.push((
                            Resource::Vram,
                            (metrics.totals.gpu_mem_bytes as f64 / vram_total as f64 * 100.0)
                                as f32,
                        ));
                    }
                }
                let crossings = debouncers
                    .entry(metrics.session_id)
                    .or_default()
                    .observe(&values);
                for crossing in crossings {
                    if let Err(error) = sessions.publish_resource_high(
                        metrics.session_id,
                        crossing.resource,
                        crossing.percent,
                        at,
                    ) {
                        tracing::warn!(session = metrics.session_id.0, %error, "failed to publish resource event");
                    }
                }
                if let Err(error) = sessions.publish_metrics(metrics) {
                    tracing::warn!(%error, "failed to publish Session metrics");
                }
            }
            if let Err(error) = store.downsample_metrics(at) {
                tracing::warn!(%error, "failed to downsample raw metrics");
            }
        }
    });
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
