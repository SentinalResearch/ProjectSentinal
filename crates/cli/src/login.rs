//! `sentinal login` / `logout` (T4.2): sign in to the control as the owner,
//! so `sentinal watch` can list and pick up Sessions on other Machines.
//!
//! The control already signs browsers in with the owner password
//! (`POST /api/v1/auth/login`) and gives back an expiring owner token; the CLI
//! gets exactly that, nothing more: 24 hours, ended by signing out
//! everywhere or changing the password. The password is read from the
//! terminal with echo off, never from argv, a pipe, or the environment. The
//! token is kept in `owner-session` (0600) next to this Machine's token and
//! sent only to the control it came from.

use protocol::{AuthLoginRequest, AuthSession, ControlProjectionSnapshot, Timestamp};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

/// Longest password read.
const MAX_PASSWORD_BYTES: usize = 1_024;
/// Longest reply read from the control.
const MAX_REPLY_BYTES: usize = 8 * 1024 * 1024;

/// Exit codes, as `connect` uses them.
pub mod exit {
    /// Signed in or out.
    pub const OK: i32 = 0;
    /// Something failed here.
    pub const INTERNAL: i32 = 1;
    /// Usage, or no terminal to read the password from.
    pub const USAGE: i32 = 2;
    /// The control could not be reached.
    pub const UNREACHABLE: i32 = 3;
    /// Wrong password, or too many tries.
    pub const REFUSED: i32 = 4;
}

/// The owner's sign-in to one control, as kept on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnerSession {
    /// The control's origin, e.g. `https://sentinal.tailnet.ts.net`.
    pub control_url: String,
    /// The owner token the control issued.
    pub token: String,
    /// When the control stops accepting it.
    pub expires_at: Timestamp,
}

/// Why the control could not be asked.
#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    /// Not signed in, or the sign-in ended.
    #[error("not signed in to the control (sentinal login)")]
    SignedOut,
    /// The control did not answer.
    #[error("the control at {0} could not be reached: {1}")]
    Unreachable(String, String),
    /// The control answered with something else.
    #[error("the control answered unexpectedly: {0}")]
    Unexpected(String),
}

/// `owner-session`, next to this Machine's token file.
pub fn session_path(token_file: &Path) -> PathBuf {
    token_file.parent().map_or_else(
        || PathBuf::from("owner-session"),
        |dir| dir.join("owner-session"),
    )
}

/// The kept sign-in, unless it is missing, unreadable, or expired at `now`.
pub fn load(path: &Path, now: Timestamp) -> Option<OwnerSession> {
    let text = std::fs::read_to_string(path).ok()?;
    let session: OwnerSession = serde_json::from_str(&text).ok()?;
    (session.expires_at.0 > now.0).then_some(session)
}

fn save(path: &Path, session: &OwnerSession) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let text = serde_json::to_vec(session)?;
    let temporary = path.with_extension("tmp");
    let _ = std::fs::remove_file(&temporary);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    file.write_all(&text)?;
    file.sync_all()?;
    std::fs::rename(&temporary, path)
}

/// A control origin the owner password may be sent to: HTTPS, or plain HTTP
/// only to this computer or a Tailscale address (the tailnet encrypts it).
/// No path, credentials, query, or fragment.
pub fn control_origin(url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| format!("'{url}' is not a URL"))?;
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("the URL must not carry credentials".into());
    }
    if parsed.query().is_some() || parsed.fragment().is_some() || parsed.path() != "/" {
        return Err("give only the control's origin, e.g. https://sentinal.tailnet.ts.net".into());
    }
    let host = parsed.host_str().unwrap_or_default();
    match parsed.scheme() {
        "https" => {}
        "http" => {
            let local = host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback() || tailscale(ip));
            if !local {
                return Err(
                    "plain http only to this computer or a Tailscale address; use https".into(),
                );
            }
        }
        _ => return Err("the control URL must be https".into()),
    }
    Ok(parsed.as_str().trim_end_matches('/').to_string())
}

/// 100.64.0.0/10, Tailscale's addresses.
fn tailscale(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.octets()[0] == 100 && (64..128).contains(&v4.octets()[1]),
        IpAddr::V6(_) => false,
    }
}

/// The control's terminal route to one Machine, as a WebSocket URL.
pub fn terminal_url(control_url: &str, machine_id: &str) -> String {
    let ws = control_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    format!("{ws}/api/v1/machines/{machine_id}/ws")
}

fn http() -> Result<reqwest::Client, RemoteError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|error| RemoteError::Unexpected(error.to_string()))
}

async fn bounded(response: reqwest::Response) -> Result<Vec<u8>, RemoteError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_REPLY_BYTES as u64)
    {
        return Err(RemoteError::Unexpected("reply too large".into()));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| RemoteError::Unexpected(error.to_string()))?;
    if bytes.len() > MAX_REPLY_BYTES {
        return Err(RemoteError::Unexpected("reply too large".into()));
    }
    Ok(bytes.to_vec())
}

/// Every Machine's Sessions from the control's read model.
pub async fn snapshot(session: &OwnerSession) -> Result<ControlProjectionSnapshot, RemoteError> {
    let url = format!("{}/api/v1/control/snapshot", session.control_url);
    let response = http()?
        .get(&url)
        .bearer_auth(&session.token)
        .send()
        .await
        .map_err(|error| {
            RemoteError::Unreachable(session.control_url.clone(), error.to_string())
        })?;
    match response.status().as_u16() {
        200 => serde_json::from_slice(&bounded(response).await?)
            .map_err(|error| RemoteError::Unexpected(error.to_string())),
        401 | 403 => Err(RemoteError::SignedOut),
        status => Err(RemoteError::Unexpected(format!("HTTP {status} from {url}"))),
    }
}

/// `sentinal login [CONTROL_URL]`.
pub async fn login(
    control_url: Option<String>,
    token_file: &Path,
    password: impl FnOnce() -> std::io::Result<Option<String>>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let Some(control_url) = control_url else {
        let _ = writeln!(
            err,
            "sentinal: this Machine is not linked to a control; give its URL: sentinal login https://…"
        );
        return exit::USAGE;
    };
    let origin = match control_origin(&control_url) {
        Ok(origin) => origin,
        Err(message) => {
            let _ = writeln!(err, "sentinal: {message}");
            return exit::USAGE;
        }
    };
    let password = match password() {
        Ok(Some(password)) if !password.is_empty() && password.len() <= MAX_PASSWORD_BYTES => {
            password
        }
        Ok(Some(_)) => {
            let _ = writeln!(err, "sentinal: no password given");
            return exit::USAGE;
        }
        Ok(None) => {
            let _ = writeln!(err, "sentinal: cancelled");
            return exit::USAGE;
        }
        Err(error) => {
            let _ = writeln!(
                err,
                "sentinal: login reads the password from a terminal: {error}"
            );
            return exit::USAGE;
        }
    };
    let client = match http() {
        Ok(client) => client,
        Err(error) => {
            let _ = writeln!(err, "sentinal: {error}");
            return exit::INTERNAL;
        }
    };
    let response = match client
        .post(format!("{origin}/api/v1/auth/login"))
        .json(&AuthLoginRequest { password })
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            let _ = writeln!(
                err,
                "sentinal: {}",
                RemoteError::Unreachable(origin, error.to_string())
            );
            return exit::UNREACHABLE;
        }
    };
    match response.status().as_u16() {
        200 => {}
        401 => {
            let _ = writeln!(err, "sentinal: wrong password");
            return exit::REFUSED;
        }
        429 => {
            let _ = writeln!(err, "sentinal: too many tries; wait a minute and try again");
            return exit::REFUSED;
        }
        status => {
            let _ = writeln!(
                err,
                "sentinal: the control refused the sign-in (HTTP {status})"
            );
            return exit::UNREACHABLE;
        }
    }
    let issued: AuthSession = match bounded(response).await.and_then(|bytes| {
        serde_json::from_slice(&bytes).map_err(|error| RemoteError::Unexpected(error.to_string()))
    }) {
        Ok(session) => session,
        Err(error) => {
            let _ = writeln!(err, "sentinal: {error}");
            return exit::UNREACHABLE;
        }
    };
    let session = OwnerSession {
        control_url: origin.clone(),
        token: issued.token,
        expires_at: issued.expires_at,
    };
    if let Err(error) = save(&session_path(token_file), &session) {
        let _ = writeln!(
            err,
            "sentinal: signed in, but the sign-in could not be kept: {error}"
        );
        return exit::INTERNAL;
    }
    let _ = writeln!(
        out,
        "Signed in to {origin} for 24 hours. `sentinal watch` now shows your other Machines."
    );
    exit::OK
}

/// `sentinal logout`: end the sign-in at the control and forget it here.
pub async fn logout(token_file: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let path = session_path(token_file);
    let Some(session) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<OwnerSession>(&text).ok())
    else {
        let _ = writeln!(out, "Not signed in.");
        return exit::OK;
    };
    let told = match http() {
        Ok(client) => client
            .post(format!("{}/api/v1/auth/logout", session.control_url))
            .bearer_auth(&session.token)
            .send()
            .await
            .is_ok_and(|response| {
                response.status().is_success() || response.status().as_u16() == 401
            }),
        Err(_) => false,
    };
    if let Err(error) = std::fs::remove_file(&path) {
        let _ = writeln!(err, "sentinal: could not forget the sign-in: {error}");
        return exit::INTERNAL;
    }
    if told {
        let _ = writeln!(out, "Signed out of {}.", session.control_url);
    } else {
        let _ = writeln!(
            out,
            "Forgot the sign-in here; the control could not be told, so it lasts until it expires (or sign out everywhere from Settings)."
        );
    }
    exit::OK
}

/// The owner password from `/dev/tty`, echo off.
pub fn read_password_from_terminal() -> std::io::Result<Option<String>> {
    let mut tty = crate::connect_command::Tty::open()?;
    match crate::connect_command::read_hidden_up_to(
        &mut tty,
        "Owner password: ",
        MAX_PASSWORD_BYTES,
    )? {
        crate::connect_command::ReadOutcome::Line(line) => Ok(Some(line)),
        crate::connect_command::ReadOutcome::Cancelled => Ok(None),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn the_password_goes_only_to_an_encrypted_control_origin() {
        assert_eq!(
            control_origin("https://sentinal.tailnet.ts.net/").unwrap(),
            "https://sentinal.tailnet.ts.net"
        );
        assert_eq!(
            control_origin("http://127.0.0.1:7530").unwrap(),
            "http://127.0.0.1:7530"
        );
        assert!(
            control_origin("http://100.90.80.70:7420").is_ok(),
            "tailnet"
        );
        assert!(control_origin("http://example.com").is_err());
        assert!(control_origin("http://192.168.1.4:7420").is_err());
        assert!(control_origin("https://user:pw@host.ts.net").is_err());
        assert!(control_origin("https://host.ts.net/path").is_err());
        assert!(control_origin("https://host.ts.net/?token=x").is_err());
        assert!(control_origin("ftp://host").is_err());
    }

    #[test]
    fn terminals_route_through_the_control_over_websocket() {
        assert_eq!(
            terminal_url("https://c.tailnet.ts.net", "m-1"),
            "wss://c.tailnet.ts.net/api/v1/machines/m-1/ws"
        );
        assert_eq!(
            terminal_url("http://127.0.0.1:7530", "m-1"),
            "ws://127.0.0.1:7530/api/v1/machines/m-1/ws"
        );
    }

    #[test]
    fn the_sign_in_is_private_and_forgotten_once_expired() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("sentinal-login-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let token_file = dir.join("token");
        let path = session_path(&token_file);
        assert_eq!(path, dir.join("owner-session"));
        let session = OwnerSession {
            control_url: "https://c.tailnet.ts.net".into(),
            token: "t".into(),
            expires_at: Timestamp(1_000),
        };
        save(&path, &session).unwrap();
        save(&path, &session).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(load(&path, Timestamp(999)), Some(session));
        assert_eq!(load(&path, Timestamp(1_000)), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn login_without_a_control_or_a_password_says_how() {
        let dir = std::env::temp_dir().join(format!("sentinal-login-usage-{}", std::process::id()));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = login(None, &dir.join("token"), || Ok(None), &mut out, &mut err).await;
        assert_eq!(code, exit::USAGE);
        assert!(String::from_utf8_lossy(&err).contains("sentinal login https://"));
        let mut err = Vec::new();
        let code = login(
            Some("https://c.tailnet.ts.net".into()),
            &dir.join("token"),
            || Ok(None),
            &mut out,
            &mut err,
        )
        .await;
        assert_eq!(code, exit::USAGE);
        assert!(String::from_utf8_lossy(&err).contains("cancelled"));
    }
}
