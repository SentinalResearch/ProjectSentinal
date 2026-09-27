//! Daemon configuration loading and network safety validation.

use serde::Deserialize;
use std::{
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

const DEFAULT_BIND: &str = "127.0.0.1:7420";
const DEFAULT_SCROLLBACK_LINES: usize = 10_000;

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
}

#[derive(Debug, Default, Deserialize)]
struct RawConfig {
    bind: Option<String>,
    data_dir: Option<PathBuf>,
    scrollback_lines: Option<usize>,
    token_file: Option<PathBuf>,
    auth_file: Option<PathBuf>,
}

/// Errors produced while loading daemon configuration.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
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
        Self::from_toml(&text, home)
    }

    /// Parse TOML configuration and resolve defaults relative to `home`.
    pub fn from_toml(text: &str, home: &Path) -> Result<Self, ConfigError> {
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

        Ok(DaemonConfig {
            bind,
            data_dir: raw
                .data_dir
                .unwrap_or_else(|| home.join(".local/share/terminal-agent")),
            scrollback_lines: raw.scrollback_lines.unwrap_or(DEFAULT_SCROLLBACK_LINES),
            token_file: raw
                .token_file
                .unwrap_or_else(|| home.join(".config/terminal-agent/token")),
            auth_file: raw
                .auth_file
                .unwrap_or_else(|| home.join(".config/terminal-agent/auth.json")),
        })
    }
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
