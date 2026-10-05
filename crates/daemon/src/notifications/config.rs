//! Owner-only `notify.toml` loading and validation.

use protocol::{NotificationConfigurationState, Timestamp};
use serde::Deserialize;
use std::{fs, os::unix::fs::MetadataExt, path::Path};

pub(super) const MAX_TOKEN_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NotifyConfig {
    pub(super) server: String,
    pub(super) topic: String,
    pub(super) token: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct ConfigSnapshot {
    pub(super) config: Option<NotifyConfig>,
    pub(super) state: NotificationConfigurationState,
    pub(super) diagnostic: Option<String>,
    pub(super) reloaded_at: Option<Timestamp>,
}

pub(super) fn load(
    path: &Path,
    owner_uid: u32,
    development: bool,
    now: Timestamp,
) -> ConfigSnapshot {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return ConfigSnapshot {
                config: None,
                state: NotificationConfigurationState::Unavailable,
                diagnostic: None,
                reloaded_at: None,
            };
        }
        Err(error) => return invalid(format!("cannot inspect file: {error}")),
    };
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return invalid("file must be a regular file, not a symlink".into());
    }
    if metadata.uid() != owner_uid {
        return invalid(format!("file must be owned by uid {owner_uid}"));
    }
    if metadata.mode() & 0o777 != 0o600 {
        return invalid("file mode must be 0600".into());
    }
    if let Some(parent) = path.parent() {
        match fs::metadata(parent) {
            Ok(parent_metadata)
                if parent_metadata.is_dir()
                    && parent_metadata.uid() == owner_uid
                    && parent_metadata.mode() & 0o077 == 0 => {}
            Ok(_) => return invalid("parent directory must be owner-only".into()),
            Err(error) => return invalid(format!("cannot inspect parent directory: {error}")),
        }
    }
    let text = match fs::read_to_string(path) {
        Ok(text) if text.len() <= 16 * 1024 => text,
        Ok(_) => return invalid("file exceeds 16384 bytes".into()),
        Err(error) => return invalid(format!("cannot read file: {error}")),
    };
    let parsed = match toml::from_str::<NotifyConfig>(&text) {
        Ok(config) => config,
        Err(error) => return invalid(format!("TOML is invalid: {error}")),
    };
    if let Err(diagnostic) = validate(&parsed, development) {
        return invalid(diagnostic);
    }
    ConfigSnapshot {
        config: Some(parsed),
        state: NotificationConfigurationState::Configured,
        diagnostic: None,
        reloaded_at: Some(now),
    }
}

fn invalid(diagnostic: String) -> ConfigSnapshot {
    ConfigSnapshot {
        config: None,
        state: NotificationConfigurationState::Invalid,
        diagnostic: Some(diagnostic),
        reloaded_at: None,
    }
}

fn validate(config: &NotifyConfig, development: bool) -> Result<(), String> {
    let url = reqwest::Url::parse(&config.server)
        .map_err(|_| "field 'server' must be an absolute URL".to_string())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err("field 'server' must not contain credentials".into());
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err("field 'server' must not contain a query or fragment".into());
    }
    match url.scheme() {
        "https" => {}
        "http" if development && private_http_host(&url) => {}
        "http" => return Err("field 'server' requires HTTPS outside development".into()),
        _ => return Err("field 'server' requires HTTPS".into()),
    }
    if config.topic.is_empty()
        || config.topic.len() > 256
        || !config
            .topic
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
    {
        return Err("field 'topic' must be a 1..=256 byte safe path segment".into());
    }
    if config.token.as_ref().is_some_and(|token| {
        token.is_empty() || token.len() > MAX_TOKEN_BYTES || token.chars().any(char::is_control)
    }) {
        return Err("field 'token' must be 1..=4096 bytes without control characters".into());
    }
    Ok(())
}

fn private_http_host(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host == "localhost" {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            let [a, b, ..] = ip.octets();
            ip.is_loopback() || (a == 100 && (64..128).contains(&b))
        }
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Err(_) => false,
    }
}
