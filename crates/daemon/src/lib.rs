//! Persistent terminal sessions and the daemon API.

/// The release this daemon was built for (`SENTINAL_RELEASE_VERSION` from the
/// release build), or the package version for development builds.
pub const DAEMON_VERSION: &str = match option_env!("SENTINAL_RELEASE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

pub mod adapter_config;
pub mod api;
pub mod audit;
pub mod auth;
pub mod config;
pub mod control;
pub mod entry;
pub mod framing;
pub mod gpu_smi;
pub mod identity;
pub mod insights;
pub mod link;
pub mod link_audit;
pub mod link_crypto;
pub mod local_socket;
pub mod locale;
pub mod log_discovery;
pub mod log_tail;
pub mod login_env;
pub mod metrics;
pub mod nodelay;
pub mod notifications;
pub mod origin;
pub mod osc;
pub mod package_service;
pub mod private_fs;
pub mod process_probe;
pub mod projection_runtime;
pub mod pty;
pub mod push;
pub mod recipes;
pub mod scrollback;
pub mod self_health;
pub mod session_manager;
pub mod shell;
pub mod storage;
pub mod tailnet_dns;
pub mod tracker;
pub mod worker_update;
