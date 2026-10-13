//! Everything `sentinal setup` does outside its own files, behind one trait so
//! tests can replace Tailscale, the service manager, and publishing.

use std::{
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use super::config::{API_BIND, PACKAGE_BIND};

/// This Machine as Tailscale reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailscaleSelf {
    /// MagicDNS name without the trailing dot, e.g. `pilot.tailnet.ts.net`.
    pub dns_name: String,
    /// Tailscale IPv4 address.
    pub ipv4: Ipv4Addr,
}

/// One Tailscale Serve route setup adds on HTTPS 443.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeRoute {
    /// `/` → the PWA and package listener.
    Root,
    /// `/api` → the daemon API.
    Api,
}

impl ServeRoute {
    /// Serve path.
    pub fn path(self) -> &'static str {
        match self {
            Self::Root => "/",
            Self::Api => "/api",
        }
    }

    /// Proxy target, exactly as `tailscale serve status --json` reports it.
    pub fn target(self) -> String {
        match self {
            Self::Root => format!("http://{PACKAGE_BIND}"),
            Self::Api => format!("http://{API_BIND}/api"),
        }
    }
}

/// Side effects of setup.
pub trait System {
    /// Tailscale's view of this Machine; `Err` when it is missing or signed out.
    fn tailscale(&mut self) -> Result<TailscaleSelf, String>;
    /// `tailscale serve status --json`.
    fn serve_status(&mut self) -> Result<serde_json::Value, String>;
    /// Add one Serve route (tailnet only; never Funnel).
    fn serve(&mut self, route: ServeRoute) -> Result<(), String>;
    /// Whether the user's services keep running after logout (`None`: not Linux).
    fn linger(&mut self, user: &str) -> Option<bool>;
    /// `sudo loginctl enable-linger USER` (asks for a password once).
    fn enable_linger(&mut self, user: &str) -> Result<(), String>;
    /// Restart the user service so it reads the new config.
    fn restart_service(&mut self) -> Result<(), String>;
    /// Whether `url` answers `{"ok":true}` within the setup deadline.
    fn healthy(&mut self, url: &str) -> bool;
    /// Publish the installed signed release to this control's package channel.
    fn publish(&mut self, version: &str, config_dir: &Path) -> Result<(), String>;
    /// Find controls on the Tailnet (4.E §4).
    fn discover(&mut self) -> Result<crate::discovery::Discovered, String>;
    /// Whether the dashboard still has no owner password (`None`: unknown).
    fn password_needed(&mut self) -> Option<bool>;
    /// Read one line without echo; `None` when cancelled.
    fn read_password(&mut self, prompt: &str) -> Option<String>;
    /// Set the first owner password with the Machine token in `token_file`.
    fn set_password(&mut self, token_file: &Path, password: &str) -> Result<(), String>;
}

/// The real Machine.
pub struct RealSystem {
    /// Installed program prefix.
    pub prefix: PathBuf,
}

const HEALTH_DEADLINE: Duration = Duration::from_secs(30);

impl RealSystem {
    fn tailscale_cli() -> Option<PathBuf> {
        if let Some(path) = std::env::var_os("SENTINAL_TAILSCALE_CLI") {
            return Some(PathBuf::from(path));
        }
        let on_path = std::env::var_os("PATH").and_then(|paths| {
            std::env::split_paths(&paths)
                .map(|dir| dir.join("tailscale"))
                .find(|path| path.is_file())
        });
        on_path.or_else(|| {
            let app = PathBuf::from("/Applications/Tailscale.app/Contents/MacOS/Tailscale");
            app.is_file().then_some(app)
        })
    }

    fn tailscale_command(&self) -> Result<Command, String> {
        let cli = Self::tailscale_cli().ok_or_else(|| {
            "Tailscale is not installed: install it from https://tailscale.com/download, sign in, then run sentinal setup".to_string()
        })?;
        let mut command = Command::new(cli);
        command.env("TAILSCALE_BE_CLI", "1").stdin(Stdio::null());
        Ok(command)
    }
}

fn output(mut command: Command) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("could not run {:?}: {error}", command.get_program()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

impl System for RealSystem {
    fn tailscale(&mut self) -> Result<TailscaleSelf, String> {
        let mut command = self.tailscale_command()?;
        command.args(["status", "--json", "--peers=false"]);
        let text = output(command)?;
        let status: serde_json::Value = serde_json::from_str(&text)
            .map_err(|error| format!("tailscale status was not JSON: {error}"))?;
        if status.get("BackendState").and_then(|state| state.as_str()) != Some("Running") {
            return Err(
                "Tailscale is not connected: run 'tailscale up', then sentinal setup".into(),
            );
        }
        let me = status.get("Self");
        let dns_name = me
            .and_then(|me| me.get("DNSName"))
            .and_then(|name| name.as_str())
            .map(|name| name.trim_end_matches('.').to_string())
            .filter(|name| name.ends_with(".ts.net"))
            .ok_or("Tailscale reports no MagicDNS name: turn on MagicDNS and HTTPS certificates for the Tailnet")?;
        let ipv4 = me
            .and_then(|me| me.get("TailscaleIPs"))
            .and_then(|ips| ips.as_array())
            .into_iter()
            .flatten()
            .filter_map(|ip| ip.as_str()?.parse::<Ipv4Addr>().ok())
            .next()
            .ok_or("Tailscale reports no IPv4 address")?;
        Ok(TailscaleSelf { dns_name, ipv4 })
    }

    fn serve_status(&mut self) -> Result<serde_json::Value, String> {
        let mut command = self.tailscale_command()?;
        command.args(["serve", "status", "--json"]);
        let text = output(command)?;
        if text.trim().is_empty() {
            return Ok(serde_json::json!({}));
        }
        serde_json::from_str(&text).map_err(|error| format!("serve status was not JSON: {error}"))
    }

    fn serve(&mut self, route: ServeRoute) -> Result<(), String> {
        let mut command = self.tailscale_command()?;
        command.args(["serve", "--bg", "--https=443"]);
        if route == ServeRoute::Api {
            command.args(["--set-path", "/api"]);
        }
        command.arg(route.target());
        output(command).map(drop).map_err(|error| {
            if error.contains("denied") || error.contains("permission") {
                let user = std::env::var("USER").unwrap_or_else(|_| "$USER".into());
                format!("Tailscale Serve was refused ({error}). Run: sudo tailscale set --operator={user}  then sentinal setup")
            } else {
                error
            }
        })
    }

    fn linger(&mut self, user: &str) -> Option<bool> {
        if !cfg!(target_os = "linux") {
            return None;
        }
        let mut command = Command::new("loginctl");
        command.args(["show-user", user, "--property=Linger", "--value"]);
        // A user with no session yet is reported as an error: not lingering.
        Some(output(command).is_ok_and(|value| value.trim() == "yes"))
    }

    fn enable_linger(&mut self, user: &str) -> Result<(), String> {
        let status = Command::new("sudo")
            .args(["loginctl", "enable-linger", user])
            .status()
            .map_err(|error| format!("could not run sudo: {error}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!(
                "Run: sudo loginctl enable-linger {user}  then sentinal setup"
            ))
        }
    }

    fn restart_service(&mut self) -> Result<(), String> {
        let mut command;
        if cfg!(target_os = "macos") {
            let uid = output({
                let mut id = Command::new("id");
                id.arg("-u");
                id
            })?;
            command = Command::new("launchctl");
            command.args([
                "kickstart",
                "-k",
                &format!("gui/{}/dev.sentinal.sentinald", uid.trim()),
            ]);
        } else {
            command = Command::new("systemctl");
            command.args(["--user", "restart", "sentinald.service"]);
        }
        output(command).map(drop)
    }

    fn healthy(&mut self, url: &str) -> bool {
        let deadline = Instant::now() + HEALTH_DEADLINE;
        loop {
            let ok = Command::new("curl")
                .args(["--fail", "--silent", "--max-time", "3", "--max-redirs", "0"])
                .arg(url)
                .stderr(Stdio::null())
                .output()
                .is_ok_and(|output| {
                    output.status.success()
                        && String::from_utf8_lossy(&output.stdout).contains("\"ok\":true")
                });
            if ok {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            thread::sleep(Duration::from_secs(1));
        }
    }

    fn publish(&mut self, version: &str, config_dir: &Path) -> Result<(), String> {
        crate::release::publish_installed(&self.prefix, config_dir, version)
            .map_err(|error| error.to_string())
    }

    fn discover(&mut self) -> Result<crate::discovery::Discovered, String> {
        let tailscale = Self::tailscale_cli().ok_or_else(|| {
            "Tailscale is not installed: install it from https://tailscale.com/download, sign in, then run sentinal setup".to_string()
        })?;
        crate::discovery::discover(
            &crate::discovery::RealTransport { tailscale },
            crate::discovery::DEADLINE,
        )
    }

    fn password_needed(&mut self) -> Option<bool> {
        let (status, body) = local_http("GET", "/api/v1/auth/status", None, None).ok()?;
        if status != 200 {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(&body).ok()?;
        value.get("setup_required")?.as_bool()
    }

    fn read_password(&mut self, prompt: &str) -> Option<String> {
        let mut tty = crate::connect_command::Tty::open().ok()?;
        match crate::connect_command::read_hidden(&mut tty, prompt) {
            Ok(crate::connect_command::ReadOutcome::Line(line)) => Some(line),
            _ => None,
        }
    }

    fn set_password(&mut self, token_file: &Path, password: &str) -> Result<(), String> {
        let token = std::fs::read_to_string(token_file)
            .map_err(|error| format!("cannot read {}: {error}", token_file.display()))?;
        let body = serde_json::json!({ "password": password }).to_string();
        let (status, reply) = local_http(
            "POST",
            "/api/v1/auth/setup",
            Some(token.trim()),
            Some(&body),
        )?;
        if status == 200 {
            return Ok(());
        }
        let message = serde_json::from_str::<serde_json::Value>(&reply)
            .ok()
            .and_then(|value| value.get("message")?.as_str().map(str::to_string))
            .unwrap_or_else(|| format!("HTTP {status}"));
        Err(message)
    }
}

/// One HTTP/1.1 request to the daemon on loopback. Nothing goes on a command
/// line, so the token and password never appear in `ps`.
fn local_http(
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    http_to(API_BIND, method, path, bearer, body)
}

fn http_to(
    address: &str,
    method: &str,
    path: &str,
    bearer: Option<&str>,
    body: Option<&str>,
) -> Result<(u16, String), String> {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect_timeout(
        &address.parse().map_err(|_| "bad API address".to_string())?,
        Duration::from_secs(3),
    )
    .map_err(|error| format!("cannot reach sentinald: {error}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let body = body.unwrap_or("");
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n");
    if let Some(bearer) = bearer {
        request.push_str(&format!("Authorization: Bearer {bearer}\r\n"));
    }
    if !body.is_empty() {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream
        .write_all(request.as_bytes())
        .map_err(|error| error.to_string())?;
    let mut reply = Vec::new();
    stream
        .take(64 * 1024)
        .read_to_end(&mut reply)
        .map_err(|error| error.to_string())?;
    let reply = String::from_utf8_lossy(&reply);
    let status = reply
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or("sentinald sent no HTTP status")?;
    let body = reply
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    Ok((status, body))
}

#[cfg(test)]
mod tests {
    use super::http_to;
    use std::io::{Read, Write};

    #[test]
    fn the_loopback_request_carries_the_bearer_and_json_body() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = vec![0_u8; 4096];
            let read = stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\n\r\n{\"message\":\"password must be between 12 and 1024 bytes\"}")
                .unwrap();
            String::from_utf8_lossy(&request[..read]).into_owned()
        });
        let (status, body) = http_to(
            &address,
            "POST",
            "/api/v1/auth/setup",
            Some("tok"),
            Some("{\"password\":\"x\"}"),
        )
        .unwrap();
        let request = server.join().unwrap();
        assert_eq!(status, 400);
        assert!(body.contains("between 12"), "{body}");
        assert!(
            request.starts_with("POST /api/v1/auth/setup HTTP/1.1\r\n"),
            "{request}"
        );
        assert!(
            request.contains("Authorization: Bearer tok\r\n"),
            "{request}"
        );
        assert!(request.contains("Content-Length: 16\r\n"), "{request}");
        assert!(
            request.ends_with("\r\n\r\n{\"password\":\"x\"}"),
            "{request}"
        );
    }
}
