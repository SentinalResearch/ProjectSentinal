//! Discovery of the daemon WebSocket URL from an installed worker config.

use std::{io::ErrorKind, net::SocketAddr, path::Path};

const LOOPBACK_URL: &str = "ws://127.0.0.1:7420/api/v1/ws";

/// Errors resolving the daemon URL from local configuration.
#[derive(Debug, thiserror::Error)]
pub enum DaemonUrlError {
    /// The daemon config could not be read.
    #[error("cannot read daemon config '{}': {source}", path.display())]
    Read {
        /// Config path.
        path: std::path::PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// The daemon config was not valid TOML.
    #[error("cannot parse daemon config '{}': {source}", path.display())]
    Parse {
        /// Config path.
        path: std::path::PathBuf,
        /// TOML error.
        source: toml::de::Error,
    },
    /// The configured bind was not a socket address.
    #[error("daemon config '{}' has invalid bind '{bind}': {source}", path.display())]
    Bind {
        /// Config path.
        path: std::path::PathBuf,
        /// Invalid bind value.
        bind: String,
        /// Address parsing error.
        source: std::net::AddrParseError,
    },
}

/// Resolve an explicit URL or derive one from the local daemon bind.
pub fn resolve(explicit: Option<&str>, config_path: &Path) -> Result<String, DaemonUrlError> {
    if let Some(url) = explicit {
        return Ok(url.to_owned());
    }
    let text = match std::fs::read_to_string(config_path) {
        Ok(text) => text,
        Err(source) if source.kind() == ErrorKind::NotFound => return Ok(LOOPBACK_URL.to_owned()),
        Err(source) => {
            return Err(DaemonUrlError::Read {
                path: config_path.to_path_buf(),
                source,
            });
        }
    };
    let config: toml::Table = toml::from_str(&text).map_err(|source| DaemonUrlError::Parse {
        path: config_path.to_path_buf(),
        source,
    })?;
    let Some(bind) = config.get("bind").and_then(toml::Value::as_str) else {
        return Ok(LOOPBACK_URL.to_owned());
    };
    let address = bind
        .parse::<SocketAddr>()
        .map_err(|source| DaemonUrlError::Bind {
            path: config_path.to_path_buf(),
            bind: bind.to_owned(),
            source,
        })?;
    Ok(format!("ws://{address}/api/v1/ws"))
}
