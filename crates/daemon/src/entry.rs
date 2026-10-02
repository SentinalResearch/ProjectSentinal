//! Startup and serving for the `sentinald` daemon (and its `tad` alias).

use crate::{
    adapter_config::{load_adapters, AdapterConfigError},
    api::{router, AppState},
    audit::AuditLog,
    auth::{load_or_create_token, AuthError, AuthManager},
    config::{ConfigError, DaemonConfig},
    control::{ControlService, ControlSettings},
    identity,
    link::{LinkManager, LinkSettings},
    local_socket::{current_uid, LocalHandler, LocalSocketError, SecurityAudit, UnixSocketServer},
    metrics::{process_tree, MetricsSampler, ResourceDebouncer},
    origin::OriginPolicy,
    package_service::{package_router, PackageService, PackageServiceError},
    private_fs::ensure_private_dir,
    process_probe::ProcessProbe,
    pty::{FakePty, PtyBackend, RealPty},
    session_manager::{SessionManager, SessionManagerConfig, SessionManagerError},
};
use protocol::{
    link::LinkEndpoint, LocalRequest, Machine, MachineId, MachineRole, MachineStatus, Os, Resource,
    Timestamp,
};
use std::{
    collections::HashMap,
    future::IntoFuture,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use store::{Store, StoreError};

/// Errors that stop the daemon from starting or serving.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    /// HOME is needed to find the default configuration.
    #[error("HOME is not set; cannot locate daemon configuration")]
    MissingHome,
    /// Configuration could not be loaded.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The bearer token or owner auth could not be loaded.
    #[error(transparent)]
    Auth(#[from] AuthError),
    /// SQLite storage failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The Session manager failed to start.
    #[error(transparent)]
    Session(#[from] SessionManagerError),
    /// The data directory could not be created or made owner-only.
    #[error("failed to create private daemon data directory '{}': {source}", path.display())]
    DataDirectory {
        /// Directory that could not be created.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// Logging could not be initialized.
    #[error("failed to initialize tracing: {0}")]
    Tracing(String),
    /// The HTTP/WebSocket listener could not bind.
    #[error("failed to bind daemon to {address}: {source}")]
    Bind {
        /// Configured listen address.
        address: SocketAddr,
        /// Socket error.
        source: std::io::Error,
    },
    /// The HTTP/WebSocket server stopped with an error.
    #[error("daemon server failed: {0}")]
    Serve(std::io::Error),
    /// The private local socket failed.
    #[error(transparent)]
    LocalSocket(#[from] LocalSocketError),
    /// The local socket task panicked or was cancelled.
    #[error("local socket task failed: {0}")]
    LocalTask(tokio::task::JoinError),
    /// The private package service could not be prepared.
    #[error(transparent)]
    Package(#[from] PackageServiceError),
    /// The private package listener could not bind.
    #[error("failed to bind package service to {address}: {source}")]
    PackageBind {
        /// Configured loopback listen address.
        address: SocketAddr,
        /// Socket error.
        source: std::io::Error,
    },
    /// The package HTTP server stopped with an error.
    #[error("package service failed: {0}")]
    PackageServe(std::io::Error),
    /// The package server task panicked or was cancelled.
    #[error("package service task failed: {0}")]
    PackageTask(tokio::task::JoinError),
    /// Allowed browser origins were invalid.
    #[error(transparent)]
    Origins(#[from] crate::origin::OriginConfigError),
    /// The control role needs a usable Machine identity.
    #[error("the control role needs a valid Machine identity: {0}")]
    Identity(String),
    /// The control role could not start.
    #[error(transparent)]
    Control(#[from] crate::control::ControlError),
    /// Declarative watcher adapters could not be loaded safely.
    #[error(transparent)]
    Adapters(#[from] AdapterConfigError),
}

/// Load configuration, start every listener, and serve until SIGINT/SIGTERM.
pub async fn run() -> Result<(), DaemonError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "daemon=info,sentinald=info,tad=info".into()),
        )
        .try_init()
        .map_err(|error| DaemonError::Tracing(error.to_string()))?;

    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or(DaemonError::MissingHome)?;
    let config_path = std::env::var_os("TA_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config/terminal-agent/daemon.toml"));
    let config = DaemonConfig::load(&config_path, &home)?;
    let token = load_or_create_token(&config.token_file)?;
    let auth_file = std::env::var_os("TA_AUTH_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| config.auth_file.clone());
    let auth = AuthManager::load(&auth_file, token)?;
    let origins = OriginPolicy::new(&config.allowed_origins, config.development)?;
    if config.allowed_origins.is_empty() {
        tracing::warn!("allowed_origins is empty: browsers cannot use this daemon (CLI and worker links still can)");
    }
    let mut machine = machine_description();
    let legacy_machine_id = machine.id.0.clone();
    let identity = identity::load_or_create(&config.identity_file, now(), &legacy_machine_id);
    match &identity {
        Ok(identity) => machine.id = identity.machine_id.clone(),
        Err(error) => {
            tracing::error!(%error, "Machine identity unusable; Machine links are disabled")
        }
    }
    if config.control.is_some() {
        machine.role = Some(MachineRole::Control);
    }
    ensure_private_dir(&config.data_dir).map_err(|source| DaemonError::DataDirectory {
        path: config.data_dir.clone(),
        source,
    })?;
    let store = Arc::new(Store::open(
        &config.data_dir.join("terminal-agent.sqlite3"),
    )?);
    if let Ok(identity) = &identity {
        if let Some(legacy) = identity.legacy_machine_id.as_deref() {
            let changed = store.rewrite_machine_id(legacy, &identity.machine_id)?;
            if changed > 0 {
                tracing::info!(changed, legacy, machine_id = %identity.machine_id, "moved history to the stable Machine ID");
            }
        }
    }
    store.upsert_machine(&machine)?;
    let control = match (&config.control, &identity) {
        (Some(control), Ok(identity)) => Some(ControlService::open(
            store.clone(),
            ControlSettings {
                public_url: control.public_url.clone(),
                heartbeat_interval_ms: control.heartbeat_interval_ms,
                development: config.development,
                machine_id: identity.machine_id.clone(),
                display_name: machine.name.clone(),
                os: machine.os,
                daemon_version: env!("CARGO_PKG_VERSION").into(),
                secrets_file: config.control_credentials_file.clone(),
                audit_dir: config.data_dir.clone(),
            },
            Arc::new(now),
        )?),
        (Some(_), Err(error)) => return Err(DaemonError::Identity(error.to_string())),
        (None, _) => None,
    };
    let link = LinkManager::open(
        LinkSettings {
            link_file: config.link_file.clone(),
            identity_file: config.identity_file.clone(),
            identity: identity.map_err(|error| error.to_string()),
            legacy_machine_id,
            hostname: machine.name.clone(),
            endpoint: LinkEndpoint {
                address: config.bind.ip().to_string(),
                port: config.bind.port(),
                dns_name: None,
            },
            os: machine.os,
            daemon_version: env!("CARGO_PKG_VERSION").into(),
            development: config.development,
            audit_dir: config.data_dir.clone(),
            background: true,
        },
        Arc::new(now),
    );
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
    let daemon_uid = current_uid(&config.local_socket)?;
    let watcher_adapters = load_adapters(&home.join(".config/terminal-agent/adapters"))?;
    let sessions = SessionManager::with_store(
        backend,
        SessionManagerConfig::system(
            machine.id.clone(),
            config.data_dir.clone(),
            config.scrollback_lines,
        ),
        AuditLog::new(&config.data_dir),
        store.clone(),
    )?
    .with_watcher_adapters(watcher_adapters)
    .with_process_probe(ProcessProbe::system(daemon_uid));
    // FULL Sessions were interrupted above; reduced-capture ones are resumed
    // only when their root process identity still matches.
    sessions.recover_external_sessions(store.live_sessions()?)?;
    spawn_metric_sampler(sessions.clone(), store);
    spawn_external_monitor(sessions.clone());
    let local_socket =
        UnixSocketServer::bind(&config.local_socket, SecurityAudit::new(&config.data_dir))?;
    let handler_sessions = sessions.clone();
    let handler_link = link.clone();
    let handler: LocalHandler = Arc::new(move |request| {
        let sessions = handler_sessions.clone();
        let link = handler_link.clone();
        Box::pin(async move {
            match request {
                LocalRequest::GetIdentity
                | LocalRequest::ConnectControl { .. }
                | LocalRequest::ConnectionStatus
                | LocalRequest::DisconnectControl
                | LocalRequest::ResetIdentity => link.handle_local(request).await,
                other => sessions.handle_local(other),
            }
        })
    });
    let state = AppState::new(sessions, machine, auth)
        .with_origins(origins)
        .with_control(control)
        .with_link(link)
        .with_tailscale_serve(config.behind_tailscale_serve);
    let (local_shutdown, local_shutdown_rx) = tokio::sync::watch::channel(false);
    let local_task = tokio::spawn(local_socket.serve(local_shutdown_rx, handler));
    let (server_shutdown, server_shutdown_rx) = tokio::sync::watch::channel(false);
    let package_task = if let Some(package_config) = config.packages.clone() {
        let package_address = package_config.listen;
        let package_origin = package_config.public_base_url.clone();
        let service = PackageService::open(package_config, daemon_uid)?;
        let package_listener = tokio::net::TcpListener::bind(package_address)
            .await
            .map_err(|source| DaemonError::PackageBind {
                address: package_address,
                source,
            })?;
        let package_shutdown = server_shutdown_rx.clone();
        tracing::info!(
            address = %package_address,
            public_origin = %package_origin,
            "private package service listening for Tailscale Serve"
        );
        Some(tokio::spawn(async move {
            axum::serve(package_listener, package_router(service))
                .with_graceful_shutdown(wait_for_shutdown(package_shutdown))
                .await
        }))
    } else {
        None
    };
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|source| DaemonError::Bind {
            address: config.bind,
            source,
        })?;
    tracing::info!(address = %config.bind, config = %config_path.display(), "daemon listening");
    let main_server = axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(wait_for_shutdown(server_shutdown_rx))
    .into_future();
    tokio::pin!(main_server);
    let signal_shutdown = server_shutdown.clone();
    let signal_task = tokio::spawn(async move {
        shutdown_signal().await;
        let _ = signal_shutdown.send(true);
    });
    let mut package_task = package_task;
    let (mut server_result, main_finished, package_finished) =
        if let Some(task) = package_task.as_mut() {
            tokio::select! {
                result = &mut main_server => (result.map_err(DaemonError::Serve), true, false),
                result = task => {
                    let result = match result {
                        Ok(Ok(())) => Ok(()),
                        Ok(Err(error)) => Err(DaemonError::PackageServe(error)),
                        Err(error) => Err(DaemonError::PackageTask(error)),
                    };
                    (result, false, true)
                }
            }
        } else {
            (
                main_server.as_mut().await.map_err(DaemonError::Serve),
                true,
                true,
            )
        };
    let _ = server_shutdown.send(true);
    let _ = local_shutdown.send(true);
    if !main_finished {
        let result = main_server.as_mut().await.map_err(DaemonError::Serve);
        if server_result.is_ok() {
            server_result = result;
        }
    }
    if !package_finished {
        if let Some(task) = package_task {
            let result = match task.await {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => Err(DaemonError::PackageServe(error)),
                Err(error) => Err(DaemonError::PackageTask(error)),
            };
            if server_result.is_ok() {
                server_result = result;
            }
        }
    }
    local_task.await.map_err(DaemonError::LocalTask)??;
    signal_task.abort();
    server_result
}

async fn wait_for_shutdown(mut shutdown: tokio::sync::watch::Receiver<bool>) {
    if *shutdown.borrow() {
        return;
    }
    let _ = shutdown.changed().await;
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

/// End external Sessions whose root process exited, checking every 2 seconds.
fn spawn_external_monitor(sessions: SessionManager) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(2));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match sessions.check_external_processes() {
                Ok(ended) if !ended.is_empty() => {
                    tracing::info!(count = ended.len(), "external shell Sessions ended");
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(%error, "failed to check external Sessions"),
            }
        }
    });
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
            let root_by_session = roots.into_iter().collect::<HashMap<_, _>>();
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
                if let Some(root) = root_by_session.get(&metrics.session_id) {
                    if let Err(error) = sessions.publish_processes(
                        metrics.session_id,
                        at,
                        process_tree(&snapshot.processes, *root),
                    ) {
                        tracing::warn!(session = metrics.session_id.0, %error, "failed to publish process observation");
                    }
                }
                let ram_percent = (ram_total > 0)
                    .then(|| (metrics.totals.rss_bytes as f64 / ram_total as f64 * 100.0) as f32);
                let session_gpu_percent = (metrics.totals.gpu_mem_bytes > 0)
                    .then_some(gpu_percent)
                    .flatten();
                let vram_percent =
                    (metrics.totals.gpu_mem_bytes > 0 && vram_total > 0).then(|| {
                        (metrics.totals.gpu_mem_bytes as f64 / vram_total as f64 * 100.0) as f32
                    });
                let mut values = vec![(Resource::Cpu, metrics.totals.cpu_percent)];
                if let Some(percent) = ram_percent {
                    values.push((Resource::Ram, percent));
                }
                if let Some(percent) = session_gpu_percent {
                    values.push((Resource::Gpu, percent));
                }
                if let Some(percent) = vram_percent {
                    values.push((Resource::Vram, percent));
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
                let resources = watcher::MetricsObservation {
                    cpu_percent: metrics.totals.cpu_percent,
                    gpu_percent: session_gpu_percent,
                    ram_percent,
                    vram_percent,
                };
                if let Err(error) = sessions.publish_metrics_with_resources(metrics, resources) {
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
        role: None,
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
