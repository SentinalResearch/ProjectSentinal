//! `sentinal release`: the control's owner-signed releases (4.E §11b). It
//! checks GitHub for releases signed by a pinned key, publishes and installs
//! them with the bundled release tools, checks health, and rolls back.

mod apply;
mod github;
mod state;

pub(crate) use apply::publish_installed;

use crate::release_args::{ReleaseAction, ReleaseChannel, ReleaseOptions};
use std::{
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
    time::{SystemTime, UNIX_EPOCH},
};

/// Release keys compiled into this binary (the repository pin).
const PINNED_RELEASE_KEYS: &str =
    include_str!("../../../../packaging/release-keys/allowed_signers");

/// Failures of `sentinal release`.
#[derive(Debug, thiserror::Error)]
pub enum ReleaseError {
    /// Release management happens on the control only.
    #[error("this Machine is a worker; update it from its control with `sentinal update`")]
    Worker,
    /// The daemon config could not be used.
    #[error("cannot use daemon config '{}': {reason}", path.display())]
    Config {
        /// Config path.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
    /// No release metadata: a source install cannot be updated this way.
    #[error("no installed release metadata at '{}'; install a signed release first", .0.display())]
    NotReleased(PathBuf),
    /// No trusted keys are available to verify releases.
    #[error("no release signing keys are pinned{0}; nothing can be trusted")]
    NoKeys(String),
    /// A version argument was malformed.
    #[error("invalid release version '{0}'; expected MAJOR.MINOR.PATCH")]
    InvalidVersion(String),
    /// A filesystem operation failed.
    #[error("failed to {action} '{}': {source}", path.display())]
    File {
        /// What was attempted.
        action: &'static str,
        /// Affected path.
        path: PathBuf,
        /// Underlying error.
        source: std::io::Error,
    },
    /// A release step failed with a specific, user-facing reason.
    #[error("{0}")]
    Refused(String),
}

/// A strict `MAJOR.MINOR.PATCH` release version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Version(u64, u64, u64);

impl Version {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let mut parts = text.split('.');
        let mut next = || -> Option<u64> {
            let part = parts.next()?;
            let valid = !part.is_empty()
                && part.len() <= 9
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            valid.then(|| part.parse().ok()).flatten()
        };
        let version = Self(next()?, next()?, next()?);
        parts.next().is_none().then_some(version)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Everything a release action needs about this control.
pub(crate) struct Context {
    pub(crate) prefix: PathBuf,
    pub(crate) data_dir: PathBuf,
    pub(crate) package_dir: PathBuf,
    pub(crate) bind: String,
    pub(crate) current: Version,
    pub(crate) repo: String,
    pub(crate) allowed_signers: PathBuf,
    pub(crate) scratch: Scratch,
}

impl Context {
    fn load(options: &ReleaseOptions) -> Result<Self, ReleaseError> {
        if !options
            .repo
            .split_once('/')
            .is_some_and(|(owner, name)| [owner, name].iter().all(|part| safe_name(part)))
        {
            return Err(ReleaseError::Refused(format!(
                "invalid repository '{}'; expected OWNER/NAME",
                options.repo
            )));
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        let config_dir = options
            .config_dir
            .clone()
            .unwrap_or_else(|| home.join(".config/terminal-agent"));
        let config_path = config_dir.join("daemon.toml");
        let invalid = |reason: &str| ReleaseError::Config {
            path: config_path.clone(),
            reason: reason.into(),
        };
        let text = fs::read_to_string(&config_path).map_err(|error| invalid(&error.to_string()))?;
        let config: toml::Value =
            toml::from_str(&text).map_err(|error| invalid(&error.to_string()))?;
        let control = config
            .get("control")
            .and_then(|control| control.get("enabled"))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false);
        if !control {
            return Err(ReleaseError::Worker);
        }
        let string = |table: Option<&toml::Value>, key: &str| {
            table
                .and_then(|table| table.get(key))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        };
        let package_dir = string(config.get("packages"), "directory")
            .map(PathBuf::from)
            .ok_or_else(|| {
                invalid("[packages] directory is not set; enable the package service")
            })?;
        let data_dir = string(Some(&config), "data_dir")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share/terminal-agent"));
        let bind = string(Some(&config), "bind").ok_or_else(|| invalid("bind is not set"))?;

        let prefix = match &options.prefix {
            Some(prefix) => prefix.clone(),
            None => installed_prefix()?,
        };
        let release_json = prefix.join("share/sentinal/release.json");
        let current = fs::read_to_string(&release_json)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|value| value.get("version")?.as_str().and_then(Version::parse))
            .ok_or_else(|| ReleaseError::NotReleased(release_json.clone()))?;

        let scratch = Scratch::create()?;
        let allowed_signers = match &options.allowed_signers {
            Some(path) => {
                let text = fs::read_to_string(path).map_err(|source| ReleaseError::File {
                    action: "read release keys",
                    path: path.clone(),
                    source,
                })?;
                if !has_key_line(&text) {
                    return Err(ReleaseError::NoKeys(format!(" in '{}'", path.display())));
                }
                path.clone()
            }
            None => {
                if !has_key_line(PINNED_RELEASE_KEYS) {
                    return Err(ReleaseError::NoKeys(" in this build".into()));
                }
                let path = scratch.path.join("allowed_signers");
                scratch.write(&path, PINNED_RELEASE_KEYS.as_bytes())?;
                path
            }
        };
        Ok(Self {
            prefix,
            data_dir,
            package_dir,
            bind,
            current,
            repo: options.repo.clone(),
            allowed_signers,
            scratch,
        })
    }

    fn tool(&self, name: &str) -> Result<PathBuf, ReleaseError> {
        let path = self.prefix.join("share/sentinal/tools/scripts").join(name);
        if path.is_file() {
            Ok(path)
        } else {
            Err(ReleaseError::Refused(format!(
                "release tool '{}' is missing; reinstall this control from a signed release",
                path.display()
            )))
        }
    }
}

/// Run one `sentinal release` action and report errors on `err`.
pub fn run(
    action: ReleaseAction,
    options: ReleaseOptions,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    match dispatch(action, &options, out) {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(err, "sentinal: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(
    action: ReleaseAction,
    options: &ReleaseOptions,
    out: &mut dyn Write,
) -> Result<ExitCode, ReleaseError> {
    // Policy needs only the data directory; still refuse on a worker.
    let context = Context::load(options)?;
    let stored = state::read_policy(&context.data_dir);
    match action {
        ReleaseAction::Check { channel, json } => {
            let channel = channel.unwrap_or(stored.channel);
            let outcome = github::check(&context, channel)?;
            state::record_check(&context, channel, &outcome)?;
            github::report(&context, channel, &outcome, json, out);
            Ok(ExitCode::SUCCESS)
        }
        ReleaseAction::Update {
            version,
            yes,
            skip_provenance,
            health_timeout,
        } => apply::update(
            &context,
            apply::UpdateRequest {
                version,
                yes,
                skip_provenance,
                health_timeout,
                channel: stored.channel,
            },
            out,
        ),
        ReleaseAction::Rollback {
            version,
            yes,
            health_timeout,
        } => apply::rollback(&context, &version, yes, health_timeout, out),
        ReleaseAction::Policy { policy, channel } => {
            let updated = state::update_policy(&context.data_dir, stored, policy, channel)?;
            let _ = writeln!(out, "{}", updated.describe());
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn channel_name(channel: ReleaseChannel) -> &'static str {
    match channel {
        ReleaseChannel::Stable => "stable",
        ReleaseChannel::Beta => "beta",
    }
}

fn safe_name(part: &str) -> bool {
    !part.is_empty()
        && part
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn has_key_line(text: &str) -> bool {
    text.lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next() == Some("sentinal-release")
            && fields.next() == Some("namespaces=\"sentinal-release\"")
            && matches!(
                fields.next(),
                Some("ssh-ed25519" | "sk-ssh-ed25519@openssh.com")
            )
            && fields.next().is_some()
    })
}

fn installed_prefix() -> Result<PathBuf, ReleaseError> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent()?.parent().map(Path::to_path_buf))
        .ok_or_else(|| {
            ReleaseError::Refused("cannot locate the install prefix; pass --prefix".into())
        })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// A private temporary directory removed on drop.
pub(crate) struct Scratch {
    pub(crate) path: PathBuf,
}

impl Scratch {
    fn create() -> Result<Self, ReleaseError> {
        let path = std::env::temp_dir().join(format!(
            "sentinal-release-{}-{}",
            std::process::id(),
            now_ms()
        ));
        fs::create_dir_all(&path).map_err(|source| ReleaseError::File {
            action: "create temporary directory",
            path: path.clone(),
            source,
        })?;
        set_mode(&path, 0o700);
        Ok(Self { path })
    }

    pub(crate) fn write(&self, path: &Path, bytes: &[u8]) -> Result<(), ReleaseError> {
        fs::write(path, bytes).map_err(|source| ReleaseError::File {
            action: "write",
            path: path.to_path_buf(),
            source,
        })
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

#[cfg(test)]
mod tests {
    use super::{has_key_line, Version};

    #[test]
    fn versions_parse_strictly_and_order_numerically() {
        assert!(Version::parse("0.1.10").unwrap() > Version::parse("0.1.9").unwrap());
        for bad in ["0.1", "0.1.18.", "01.1.1", "v0.1.1", "0.1.x", "0.1.1.1", ""] {
            assert!(Version::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn key_lines_need_the_sentinal_release_identity_and_namespace() {
        assert!(has_key_line(
            "sentinal-release namespaces=\"sentinal-release\" ssh-ed25519 AAAA k"
        ));
        assert!(!has_key_line("# comment only\n"));
        assert!(!has_key_line(
            "someone namespaces=\"sentinal-release\" ssh-ed25519 AAAA"
        ));
    }
}
