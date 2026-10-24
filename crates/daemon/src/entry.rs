//! Startup and serving for the `sentinald` daemon (and its `tad` alias).

use crate::{
    adapter_config::{load_adapters, AdapterConfigError},
    api::{router, AppState},
    audit::AuditLog,
    auth::{load_or_create_token, AuthError, AuthManager},
    config::{ConfigError, DaemonConfig},
    control::{
        release_watch::{spawn_release_watch, ReleaseWatchSettings},
        ControlService, ControlSettings,
    },
    identity,
    insights::{HostFacts, MachineInsights},
    link::{
        spawn_notification_intent_uploader, spawn_projection_uploader, LinkManager, LinkSettings,
    },
    local_socket::{current_uid, LocalHandler, LocalSocketError, SecurityAudit, UnixSocketServer},
    metrics::{process_tree, MetricsSampler, ResourceDebouncer},
    notifications::{
        spawn_notification_workers, spawn_notifier, NativeNotificationState, NotificationService,
    },
    origin::OriginPolicy,
    package_service::{package_router, PackageService, PackageServiceError},
    private_fs::ensure_private_dir,
    process_probe::ProcessProbe,
    projection_runtime::spawn_projection_recorder,
    pty::{FakePty, PtyBackend, RealPty},
    push::delivery::spawn_push_delivery_worker,
    push::gateway::{PushGateway, PushGatewaySettings},
    recipes::RecipeCatalog,
    session_manager::{SessionManager, SessionManagerConfig, SessionManagerError},
};
use protocol::{
    link::{LinkEndpoint, LocalLinkState},
    LocalRequest, Machine, MachineId, MachineRole, MachineStatus, NotificationIntentBatch, Os,
    Resource, Timestamp,
};
use std::{
    collections::HashMap,
    future::IntoFuture,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use store::{NewNotificationIntentOutbox, Store, StoreError};

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
        // Covers the recorded legacy ID and any older hostname fallback.
        let changed = store.adopt_local_history(&identity.machine_id)?;
        if changed > 0 {
            tracing::info!(changed, machine_id = %identity.machine_id, "moved history to the stable Machine ID");
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
                discovery_name: control.display_name.clone(),
                package_dir: config
                    .packages
                    .as_ref()
                    .map(|packages| packages.directory.clone()),
                os: machine.os,
                daemon_version: crate::DAEMON_VERSION.into(),
                secrets_file: config.control_credentials_file.clone(),
                audit_dir: config.data_dir.clone(),
                read_model_enabled: config.control_read_model,
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
            daemon_version: crate::DAEMON_VERSION.into(),
            development: config.development,
            audit_dir: config.data_dir.clone(),
            background: true,
        },
        Arc::new(now),
    )
    .with_projection_store(store.clone());
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
    let push = config.control.as_ref().map(|control| {
        let settings = PushGatewaySettings {
            key_file: push_key_file(&home),
            subject_origin: control.public_url.clone(),
            owner_uid: daemon_uid,
        };
        #[cfg(feature = "e2e-test-support")]
        if std::env::var_os("SENTINAL_E2E_ACCEPT_PUSH").as_deref()
            == Some(std::ffi::OsStr::new("1"))
        {
            tracing::warn!("using compile-time Web Push acceptance transport");
            return crate::push::e2e::accepted_gateway(store.clone(), settings, Arc::new(now));
        }
        PushGateway::new(store.clone(), settings, Arc::new(now))
    });
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
    let insights = MachineInsights::new(
        HostFacts::detect(config.bind.to_string(), now()),
        Some(store.clone()),
    );
    spawn_metric_sampler(
        sessions.clone(),
        store.clone(),
        insights.clone(),
        machine.cpu_cores,
    );
    spawn_external_monitor(sessions.clone());
    let local_socket =
        UnixSocketServer::bind(&config.local_socket, SecurityAudit::new(&config.data_dir))?;
    let handler_sessions = sessions.clone();
    let handler_link = link.clone();
    let handler_control = control.clone();
    let handler: LocalHandler = Arc::new(move |request| {
        let sessions = handler_sessions.clone();
        let link = handler_link.clone();
        let control = handler_control.clone();
        Box::pin(async move {
            match request {
                LocalRequest::HandoffWorkers { target, dry_run } => {
                    let Some(control) = control else {
                        return protocol::LocalResponse::Error {
                            code: protocol::LocalErrorCode::InvalidRequest,
                            message: "this Machine is not a control".into(),
                        };
                    };
                    match control.handoff_workers(target.as_ref(), dry_run).await {
                        Ok(workers) => protocol::LocalResponse::Handoff { workers },
                        Err(message) => protocol::LocalResponse::Error {
                            code: protocol::LocalErrorCode::InvalidRequest,
                            message,
                        },
                    }
                }
                LocalRequest::GetIdentity
                | LocalRequest::ConnectControl { .. }
                | LocalRequest::ConnectionStatus
                | LocalRequest::DisconnectControl
                | LocalRequest::RequestApproval { .. }
                | LocalRequest::ClaimApproval
                | LocalRequest::CancelApproval
                | LocalRequest::ResetIdentity => link.handle_local(request).await,
                other => sessions.handle_local(other),
            }
        })
    });
    let shutdown_sessions = sessions.clone();
    let (recipes, recipe_diagnostics) =
        RecipeCatalog::load(Some(home.join(".config/terminal-agent/recipes")));
    for diagnostic in recipe_diagnostics {
        tracing::warn!(path = %diagnostic.path.display(), reason = %diagnostic.message, "Recipe file ignored or overridden");
    }
    let configured_control_origin = config
        .control
        .as_ref()
        .map(|value| value.public_url.clone());
    let notification_link = link.clone();
    let control_origin = Arc::new(move || {
        configured_control_origin.clone().or_else(|| {
            notification_link.status().ok().and_then(|report| {
                matches!(
                    report.state,
                    LocalLinkState::Active | LocalLinkState::ControlUnreachable
                )
                .then_some(report.control_url)
                .flatten()
            })
        })
    });
    let (notifications, notification_diagnostics) = NotificationService::load(
        &config.notify_file,
        config.data_dir.join("notification-rules.json"),
        machine.id.clone(),
        machine.name.clone(),
        daemon_uid,
        config.development,
        store.clone(),
        control_origin,
        Arc::new(now),
    );
    let native_store = store.clone();
    let native_machine_id = machine.id.clone();
    let native_link = link.clone();
    let native_push = push.clone();
    let notifications = notifications.with_native_sink(Arc::new(move |intent, at| {
        if native_push
            .as_ref()
            .is_some_and(PushGateway::notification_ingress)
        {
            native_store
                .accept_notification_intents(
                    &NotificationIntentBatch {
                        machine_id: native_machine_id.clone(),
                        sent_at: at,
                        intents: vec![intent.clone()],
                    },
                    at,
                    7 * 24 * 60 * 60 * 1_000,
                    5 * 60 * 1_000,
                )
                .map_err(|error| error.to_string())?;
            return Ok(Some(NativeNotificationState::ControlAccepted));
        }
        if native_push
            .as_ref()
            .is_some_and(|gateway| gateway.provider_mode() == protocol::PushProviderMode::Disabled)
        {
            return Ok(Some(NativeNotificationState::Disabled));
        }
        if native_link.notification_ingress() {
            native_store
                .record_notification_intent_outbox(&NewNotificationIntentOutbox {
                    machine_id: native_machine_id.clone(),
                    intent: intent.clone(),
                    at,
                    pending_limit: 256,
                    pending_bytes_limit: 512 * 1024,
                })
                .map_err(|error| error.to_string())?;
            return Ok(Some(NativeNotificationState::ForwardQueued));
        }
        Ok(None)
    }));
    for diagnostic in notification_diagnostics {
        tracing::warn!(reason = %diagnostic, "notification configuration ignored");
    }
    spawn_notifier(
        notifications.clone(),
        sessions.subscribe_events(),
        store.clone(),
        machine.name.clone(),
    );
    let (server_shutdown, server_shutdown_rx) = tokio::sync::watch::channel(false);
    spawn_notification_workers(notifications.clone(), server_shutdown_rx.clone());
    spawn_notification_intent_uploader(link.clone(), store.clone(), server_shutdown_rx.clone());
    spawn_projection_recorder(
        sessions.clone(),
        store.clone(),
        machine.id.clone(),
        control.clone(),
        server_shutdown_rx.clone(),
    );
    spawn_projection_uploader(link.clone(), store.clone(), server_shutdown_rx.clone());
    if let Some(control) = control.clone() {
        // Runs until shutdown; nothing waits for it.
        drop(crate::control::health_monitor::spawn(
            control,
            server_shutdown_rx.clone(),
        ));
    }
    crate::self_health::spawn(
        config.data_dir.clone(),
        sessions.clone(),
        link.clone(),
        control.clone(),
        std::sync::Arc::new(now),
        server_shutdown_rx.clone(),
    );
    if let Some(gateway) = push.clone() {
        spawn_push_delivery_worker(gateway, server_shutdown_rx.clone());
    }
    crate::storage::spawn_storage_retention(
        config.data_dir.clone(),
        store.clone(),
        config.storage,
        server_shutdown_rx.clone(),
    );
    let mut release = None;
    if config.control.is_some() {
        match sibling_sentinal_bin() {
            Ok(sentinal_bin) => {
                let service = control_release(
                    &config,
                    &config_path,
                    sentinal_bin.clone(),
                    sessions.clone(),
                );
                spawn_release_watch(
                    ReleaseWatchSettings {
                        data_dir: config.data_dir.clone(),
                        sentinal_bin,
                    },
                    Some(crate::control::release_watch::ReleaseActors {
                        release: service.clone(),
                        push: push.clone(),
                    }),
                    server_shutdown_rx.clone(),
                );
                release = Some(service);
            }
            Err(error) => {
                tracing::error!(%error, "release watch disabled: cannot locate the sentinal CLI binary");
            }
        }
    }
    let worker_updater = if config.control.is_none() {
        sibling_sentinal_bin().ok().map(|sentinal_bin| {
            let updater = worker_updater(&config, sentinal_bin, sessions.clone());
            let _ = link.clone().with_updater(updater.clone());
            updater
        })
    } else {
        None
    };
    let mut state = AppState::new(sessions, machine, auth)
        .with_insights(insights)
        .with_origins(origins)
        .with_control(control)
        .with_link(link)
        .with_recipes(recipes)
        .with_notifications(notifications)
        .with_push(push)
        .with_release(release)
        .with_tailscale_serve(config.behind_tailscale_serve)
        .with_development(config.development);
    if let Some(updater) = worker_updater {
        state = state.with_worker_updater(updater);
    }
    let (local_shutdown, local_shutdown_rx) = tokio::sync::watch::channel(false);
    let local_task = tokio::spawn(local_socket.serve(local_shutdown_rx, handler));
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
        crate::nodelay::listener(listener),
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
    // End live PTYs so their blocking readers cannot keep the process alive.
    let stopped = shutdown_sessions.shutdown();
    if stopped > 0 {
        tracing::info!(stopped, "interrupted live Sessions for shutdown");
    }
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

fn push_key_file(home: &std::path::Path) -> PathBuf {
    #[cfg(feature = "e2e-test-support")]
    if let Some(path) = std::env::var_os("SENTINAL_E2E_VAPID_KEY_FILE") {
        return PathBuf::from(path);
    }
    home.join(".config/terminal-agent/push-vapid.json")
}

/// The `sentinal` CLI binary installed beside this running `sentinald`.
/// The dashboard's release service: it runs the sibling `sentinal` CLI and
/// counts live FULL Sessions an update would interrupt.
/// Live FULL Sessions on this Machine (an update or restart interrupts them).
fn live_full_sessions(sessions: SessionManager) -> std::sync::Arc<dyn Fn() -> u32 + Send + Sync> {
    std::sync::Arc::new(move || {
        sessions.list().map_or(0, |list| {
            list.iter()
                .filter(|session| {
                    session.capture == protocol::CaptureLevel::Full && !session.state.is_finished()
                })
                .count() as u32
        })
    })
}

fn detach() -> crate::control::Detach {
    // systemd stops a unit's whole cgroup on restart; the updater needs its own unit.
    if cfg!(target_os = "linux") && which("systemd-run") {
        crate::control::Detach::SystemdRun
    } else {
        crate::control::Detach::ProcessGroup
    }
}

fn worker_updater(
    config: &DaemonConfig,
    sentinal_bin: PathBuf,
    sessions: SessionManager,
) -> crate::worker_update::WorkerUpdater {
    crate::worker_update::WorkerUpdater {
        data_dir: config.data_dir.clone(),
        cli: sentinal_bin,
        detach: detach(),
        daemon_version: crate::DAEMON_VERSION.into(),
        live_full_sessions: live_full_sessions(sessions),
        clock: Arc::new(now),
    }
}

fn control_release(
    config: &DaemonConfig,
    config_path: &Path,
    sentinal_bin: PathBuf,
    sessions: SessionManager,
) -> crate::control::ControlRelease {
    let prefix = sentinal_bin
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let detach = detach();
    crate::control::ControlRelease::new(crate::control::ControlReleaseSettings {
        data_dir: config.data_dir.clone(),
        config_dir: config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default(),
        prefix,
        cli: sentinal_bin,
        detach,
        live_full_sessions: live_full_sessions(sessions),
    })
}

fn which(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

fn sibling_sentinal_bin() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let dir = exe
        .parent()
        .ok_or_else(|| "the running binary has no parent directory".to_string())?;
    Ok(dir.join("sentinal"))
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

fn spawn_metric_sampler(
    sessions: SessionManager,
    store: Arc<Store>,
    insights: MachineInsights,
    logical_cores: u32,
) {
    tokio::spawn(async move {
        let mut sampler = MetricsSampler::new();
        let mut debouncers = HashMap::<protocol::SessionId, ResourceDebouncer>::new();
        let mut last_health_prune = i64::MIN;
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
            insights.record_processes(
                root_by_session
                    .iter()
                    .map(|(session_id, root)| {
                        (*session_id, process_tree(&snapshot.processes, *root))
                    })
                    .collect(),
            );
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
                // Alerts and health judge CPU as a share of the Machine; the
                // per-core sum (top's convention) can pass 100% on many cores.
                let cpu_share =
                    crate::metrics::machine_cpu_share(metrics.totals.cpu_percent, logical_cores);
                let mut values = vec![(Resource::Cpu, cpu_share)];
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
                    cpu_machine_percent: Some(cpu_share),
                    gpu_percent: session_gpu_percent,
                    ram_percent,
                    vram_percent,
                };
                if let Err(error) = sessions.publish_metrics_with_resources(metrics, resources) {
                    tracing::warn!(%error, "failed to publish Session metrics");
                }
            }
            if let Err(error) = store.downsample_metrics(at) {
                crate::self_health::COUNTERS
                    .error(protocol::observability::HealthLabel::ErrorStore);
                tracing::warn!(%error, "failed to downsample raw metrics");
            }
            // Health retention (4.H) needs minute resolution, not every tick.
            if at.0.saturating_sub(last_health_prune) >= 60_000 {
                last_health_prune = at.0;
                if let Err(error) = store.prune_health(at) {
                    tracing::warn!(%error, "failed to prune health data");
                }
            }
        }
    });
}

fn machine_description() -> Machine {
    let name = machine_name();
    let (cpu, ram_bytes) = crate::metrics::host_hardware();
    Machine {
        id: MachineId(slug(&name)),
        name,
        os: operating_system(),
        cpu,
        cpu_cores: std::thread::available_parallelism()
            .map(|cores| u32::try_from(cores.get()).unwrap_or(u32::MAX))
            .unwrap_or(1),
        ram_bytes,
        gpus: crate::gpu_smi::inventory(),
        status: MachineStatus::Online,
        last_seen: now(),
        role: None,
    }
}

fn machine_name() -> String {
    pick_hostname(
        std::env::var("HOSTNAME").ok(),
        std::fs::read_to_string(Path::new("/etc/hostname")).ok(),
        sysinfo::System::host_name(),
    )
}

/// First non-empty of `$HOSTNAME`, `/etc/hostname` (Linux), and the OS
/// hostname (macOS has neither of the first two).
pub fn pick_hostname(env: Option<String>, etc: Option<String>, system: Option<String>) -> String {
    [env, etc, system]
        .into_iter()
        .flatten()
        .map(|name| name.trim().to_string())
        .find(|name| !name.is_empty())
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

#[cfg(test)]
mod hostname_tests {
    use super::pick_hostname;

    #[test]
    fn macos_falls_back_to_the_os_hostname() {
        assert_eq!(
            pick_hostname(None, None, Some("Mac-mini.local\n".into())),
            "Mac-mini.local"
        );
        assert_eq!(
            pick_hostname(Some(" ".into()), None, Some("mini".into())),
            "mini"
        );
    }

    #[test]
    fn explicit_sources_win_and_nothing_gives_the_default() {
        assert_eq!(
            pick_hostname(Some("env".into()), Some("etc".into()), Some("os".into())),
            "env"
        );
        assert_eq!(
            pick_hostname(None, Some("pop-os\n".into()), Some("os".into())),
            "pop-os"
        );
        assert_eq!(pick_hostname(None, None, None), "terminal-agent-machine");
    }
}
