//! Daemon configuration loading and network safety validation.

use serde::Deserialize;
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

const DEFAULT_BIND: &str = "127.0.0.1:7420";
const DEFAULT_SCROLLBACK_LINES: usize = 10_000;
const DEFAULT_PACKAGE_BIND: &str = "127.0.0.1:7422";
const DEFAULT_MAX_ARTIFACT_BYTES: u64 = 256 * 1024 * 1024;

/// Configuration for the optional Tailnet package listener.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageServiceConfig {
    /// Loopback address consumed by Tailscale Serve.
    pub listen: SocketAddr,
    /// Private directory containing published manifests and artifacts.
    pub directory: PathBuf,
    /// Tailnet HTTPS origin advertised by generated bootstrap scripts.
    pub public_base_url: String,
    /// Maximum accepted artifact length.
    pub max_artifact_bytes: u64,
    /// Optional production PWA build served from the same Tailnet HTTPS origin.
    pub web_root: Option<PathBuf>,
    /// Optional owner-only file adding release keys to the compiled-in pin
    /// (development keys, or an emergency rotation before a signed update).
    pub extra_release_keys: Option<PathBuf>,
}

/// Configuration of the owner-selected control Machine role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlConfig {
    /// Origin workers and owners use, e.g. `https://sentinal-control.<tailnet>.ts.net`.
    pub public_url: String,
    /// Heartbeat interval sent to workers (shorter values are development-only).
    pub heartbeat_interval_ms: u64,
}

/// Default worker heartbeat interval.
pub const DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = 30_000;

/// Configuration used to start the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    /// Localhost or Tailscale address on which the daemon listens.
    pub bind: SocketAddr,
    /// Directory containing session logs and the audit log.
    pub data_dir: PathBuf,
    /// Maximum number of terminal lines retained in memory per session.
    pub scrollback_lines: usize,
    /// File containing the bearer token.
    pub token_file: PathBuf,
    /// File containing the single owner's password hash.
    pub auth_file: PathBuf,
    /// Owner-only ntfy configuration for this Machine.
    pub notify_file: PathBuf,
    /// Private same-user Unix socket for local reporting.
    pub local_socket: PathBuf,
    /// Optional private package service for unpaired workers.
    pub packages: Option<PackageServiceConfig>,
    /// Immutable Machine identity file.
    pub identity_file: PathBuf,
    /// This worker's control link (secrets, owner-only).
    pub link_file: PathBuf,
    /// Control-side secrets: pairing key and credentials presented to workers.
    pub control_credentials_file: PathBuf,
    /// Exact browser origins allowed on HTTP and WebSocket routes.
    pub allowed_origins: Vec<String>,
    /// Development mode: plain-HTTP origins and control URLs, short heartbeats.
    pub development: bool,
    /// The API is published by Tailscale Serve, which sets `Tailscale-User-Login`.
    pub behind_tailscale_serve: bool,
    /// Present when this daemon is the owner-selected control Machine.
    pub control: Option<ControlConfig>,
    /// Whether the control serves and accepts the disposable dashboard read model.
    pub control_read_model: bool,
    /// Session-log retention limits (`[storage]`).
    pub storage: crate::storage::StorageLimits,
}

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    bind: Option<String>,
    data_dir: Option<PathBuf>,
    scrollback_lines: Option<usize>,
    token_file: Option<PathBuf>,
    auth_file: Option<PathBuf>,
    notify_file: Option<PathBuf>,
    local_socket: Option<PathBuf>,
    packages: Option<RawPackageConfig>,
    identity_file: Option<PathBuf>,
    link_file: Option<PathBuf>,
    control_credentials_file: Option<PathBuf>,
    #[serde(default)]
    allowed_origins: Vec<String>,
    #[serde(default)]
    development: bool,
    #[serde(default)]
    behind_tailscale_serve: bool,
    #[serde(default = "default_control_read_model")]
    control_read_model: bool,
    control: Option<RawControlConfig>,
    storage: Option<RawStorageConfig>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStorageConfig {
    session_log_max_bytes: Option<u64>,
    session_log_max_age_days: Option<u64>,
}

fn default_control_read_model() -> bool {
    true
}

#[derive(Debug, Default, Deserialize)]
struct RawControlConfig {
    #[serde(default)]
    enabled: bool,
    public_url: Option<String>,
    heartbeat_interval_ms: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
struct RawPackageConfig {
    #[serde(default)]
    enabled: bool,
    listen: Option<String>,
    directory: Option<PathBuf>,
    public_base_url: Option<String>,
    max_artifact_bytes: Option<u64>,
    web_root: Option<PathBuf>,
    extra_release_keys: Option<PathBuf>,
}

/// Errors produced while loading daemon configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A `[storage]` limit was zero.
    #[error("invalid [storage] setting: {0} must be at least 1")]
    InvalidStorage(&'static str),
    /// The configuration file could not be read.
    #[error("failed to read daemon config '{}': {source}", path.display())]
    Read {
        /// Configuration path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// TOML syntax or values were invalid.
    #[error("failed to parse daemon config: {0}")]
    Parse(#[from] toml::de::Error),
    /// The bind value is not a socket address.
    #[error("invalid daemon bind address '{value}': {source}")]
    InvalidBind {
        /// Invalid configuration value.
        value: String,
        /// Address parser error.
        source: std::net::AddrParseError,
    },
    /// Binding on every interface is forbidden.
    #[error("refusing unsafe daemon bind address '{0}'; use localhost or a Tailscale IP")]
    UnsafeBind(String),
    /// The package listener was not loopback-only.
    #[error("refusing unsafe package bind address '{0}'; Tailscale Serve must proxy to loopback")]
    UnsafePackageBind(String),
    /// A required package setting was absent.
    #[error("package service is enabled but '{0}' is missing")]
    MissingPackageField(&'static str),
    /// The package listener value is not a socket address.
    #[error("invalid package listen address '{value}': {source}")]
    InvalidPackageBind {
        /// Invalid configuration value.
        value: String,
        /// Address parser error.
        source: std::net::AddrParseError,
    },
    /// The public package origin was unsafe or malformed.
    #[error("invalid package public_base_url '{value}': {reason}")]
    InvalidPackageUrl {
        /// Invalid URL.
        value: String,
        /// Validation failure.
        reason: &'static str,
    },
    /// An allowed origin was unsafe or malformed.
    #[error(transparent)]
    InvalidOrigin(#[from] crate::origin::OriginConfigError),
    /// The control section was enabled without a valid public URL.
    #[error("invalid [control] public_url: {0}")]
    InvalidControlUrl(String),
    /// A control heartbeat interval outside the allowed range.
    #[error("invalid [control] heartbeat_interval_ms {0}; production requires 30000..=300000")]
    InvalidHeartbeat(u64),
    /// The package artifact limit was outside the supported range.
    #[error("invalid package max_artifact_bytes {0}; expected 1..={DEFAULT_MAX_ARTIFACT_BYTES}")]
    InvalidPackageSize(u64),
}

impl DaemonConfig {
    /// Load a configuration file, using defaults when it does not exist.
    pub fn load(path: &Path, home: &Path) -> Result<Self, ConfigError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        Self::from_toml_with_runtime(
            &text,
            home,
            std::env::var_os("XDG_RUNTIME_DIR")
                .as_deref()
                .map(Path::new),
        )
    }

    /// Parse TOML configuration and resolve defaults relative to `home`.
    pub fn from_toml(text: &str, home: &Path) -> Result<Self, ConfigError> {
        Self::from_toml_with_runtime(text, home, None)
    }

    /// Parse configuration with an injected Linux runtime directory.
    pub fn from_toml_with_runtime(
        text: &str,
        home: &Path,
        xdg_runtime_dir: Option<&Path>,
    ) -> Result<Self, ConfigError> {
        let raw: RawConfig = toml::from_str(text)?;
        let bind_text = raw.bind.unwrap_or_else(|| DEFAULT_BIND.to_string());
        let bind: SocketAddr = bind_text
            .parse()
            .map_err(|source| ConfigError::InvalidBind {
                value: bind_text.clone(),
                source,
            })?;
        if !is_safe_bind_ip(bind.ip()) {
            return Err(ConfigError::UnsafeBind(bind_text));
        }

        let data_dir = raw
            .data_dir
            .unwrap_or_else(|| home.join(protocol::local::DEFAULT_DATA_DIR));
        let local_socket = crate::local_socket::default_socket_path(
            &data_dir,
            raw.local_socket.as_deref(),
            xdg_runtime_dir,
            cfg!(target_os = "macos"),
        );
        let packages = parse_package_config(raw.packages)?;
        crate::origin::OriginPolicy::new(&raw.allowed_origins, raw.development)?;
        let control = parse_control_config(raw.control, raw.development)?;
        let config_dir = home.join(".config/terminal-agent");

        Ok(DaemonConfig {
            bind,
            data_dir,
            scrollback_lines: raw.scrollback_lines.unwrap_or(DEFAULT_SCROLLBACK_LINES),
            token_file: raw
                .token_file
                .unwrap_or_else(|| home.join(".config/terminal-agent/token")),
            auth_file: raw
                .auth_file
                .unwrap_or_else(|| home.join(".config/terminal-agent/auth.json")),
            notify_file: raw
                .notify_file
                .unwrap_or_else(|| config_dir.join("notify.toml")),
            local_socket,
            packages,
            identity_file: raw
                .identity_file
                .unwrap_or_else(|| config_dir.join("machine-id.json")),
            link_file: raw
                .link_file
                .unwrap_or_else(|| config_dir.join("link.json")),
            control_credentials_file: raw
                .control_credentials_file
                .unwrap_or_else(|| config_dir.join("machine-credentials.json")),
            allowed_origins: raw.allowed_origins,
            development: raw.development,
            behind_tailscale_serve: raw.behind_tailscale_serve,
            control,
            control_read_model: raw.control_read_model,
            storage: parse_storage(raw.storage)?,
        })
    }
}

fn parse_control_config(
    raw: Option<RawControlConfig>,
    development: bool,
) -> Result<Option<ControlConfig>, ConfigError> {
    let Some(raw) = raw.filter(|value| value.enabled) else {
        return Ok(None);
    };
    let policy = if development {
        protocol::link::ControlUrlPolicy::Development
    } else {
        protocol::link::ControlUrlPolicy::Production
    };
    let value = raw
        .public_url
        .ok_or_else(|| ConfigError::InvalidControlUrl("public_url is required".into()))?;
    let public_url = protocol::link::validate_control_url(&value, policy)
        .map_err(|error| ConfigError::InvalidControlUrl(error.to_string()))?;
    let heartbeat_interval_ms = raw
        .heartbeat_interval_ms
        .unwrap_or(DEFAULT_HEARTBEAT_INTERVAL_MS);
    let allowed = if development {
        100..=300_000
    } else {
        30_000..=300_000
    };
    if !allowed.contains(&heartbeat_interval_ms) {
        return Err(ConfigError::InvalidHeartbeat(heartbeat_interval_ms));
    }
    Ok(Some(ControlConfig {
        public_url,
        heartbeat_interval_ms,
    }))
}

fn parse_package_config(
    raw: Option<RawPackageConfig>,
) -> Result<Option<PackageServiceConfig>, ConfigError> {
    let Some(raw) = raw.filter(|value| value.enabled) else {
        return Ok(None);
    };
    let listen_text = raw
        .listen
        .unwrap_or_else(|| DEFAULT_PACKAGE_BIND.to_string());
    let listen: SocketAddr =
        listen_text
            .parse()
            .map_err(|source| ConfigError::InvalidPackageBind {
                value: listen_text.clone(),
                source,
            })?;
    if !listen.ip().is_loopback() {
        return Err(ConfigError::UnsafePackageBind(listen_text));
    }
    let directory = raw
        .directory
        .ok_or(ConfigError::MissingPackageField("directory"))?;
    let public_base_url = raw
        .public_base_url
        .ok_or(ConfigError::MissingPackageField("public_base_url"))?;
    validate_package_url(&public_base_url)?;
    let max_artifact_bytes = raw.max_artifact_bytes.unwrap_or(DEFAULT_MAX_ARTIFACT_BYTES);
    if max_artifact_bytes == 0 || max_artifact_bytes > DEFAULT_MAX_ARTIFACT_BYTES {
        return Err(ConfigError::InvalidPackageSize(max_artifact_bytes));
    }
    Ok(Some(PackageServiceConfig {
        listen,
        directory,
        public_base_url,
        max_artifact_bytes,
        web_root: raw.web_root,
        extra_release_keys: raw.extra_release_keys,
    }))
}

fn validate_package_url(value: &str) -> Result<(), ConfigError> {
    let Some(host) = value.strip_prefix("https://") else {
        return Err(ConfigError::InvalidPackageUrl {
            value: value.to_string(),
            reason: "production package URLs must use HTTPS",
        });
    };
    let safe_host = !host.is_empty()
        && host.ends_with(".ts.net")
        && !host.contains(['/', '?', '#', '@', ':'])
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'));
    if !safe_host {
        return Err(ConfigError::InvalidPackageUrl {
            value: value.to_string(),
            reason: "expected one credential-free .ts.net origin with no path or port",
        });
    }
    Ok(())
}

fn is_safe_bind_ip(ip: IpAddr) -> bool {
    if ip.is_loopback() {
        return true;
    }
    match ip {
        IpAddr::V4(address) => is_tailscale_ipv4(address),
        IpAddr::V6(address) => address.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
    }
}

fn is_tailscale_ipv4(address: Ipv4Addr) -> bool {
    const CGNAT_PREFIX: u32 = 0x6440_0000;
    const CGNAT_MASK: u32 = 0xffc0_0000;
    u32::from(address) & CGNAT_MASK == CGNAT_PREFIX
}

fn parse_storage(
    raw: Option<RawStorageConfig>,
) -> Result<crate::storage::StorageLimits, ConfigError> {
    let defaults = crate::storage::StorageLimits::default();
    let raw = raw.unwrap_or_default();
    let limits = crate::storage::StorageLimits {
        session_log_max_bytes: raw
            .session_log_max_bytes
            .unwrap_or(defaults.session_log_max_bytes),
        session_log_max_age_days: raw
            .session_log_max_age_days
            .unwrap_or(defaults.session_log_max_age_days),
    };
    if limits.session_log_max_bytes == 0 {
        return Err(ConfigError::InvalidStorage("session_log_max_bytes"));
    }
    if limits.session_log_max_age_days == 0 {
        return Err(ConfigError::InvalidStorage("session_log_max_age_days"));
    }
    Ok(limits)
}
