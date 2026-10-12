//! The CONTROL `daemon.toml` edit. Text-based so the owner's comments and
//! every other key survive: the top-level `bind` moves to loopback, the Serve
//! keys go above the first table, and `[control]`/`[packages]` are appended.
//! Anything set by hand that setup would have to change is refused instead.

use std::path::Path;

/// API listener behind Tailscale Serve `/api`.
pub const API_BIND: &str = "127.0.0.1:7420";
/// PWA and package listener behind Tailscale Serve `/`.
pub const PACKAGE_BIND: &str = "127.0.0.1:7422";

/// Values a control config needs.
pub struct ControlInputs<'a> {
    /// `https://<MagicDNS name>`.
    pub origin: &'a str,
    /// Name shown to workers that discover this control.
    pub display_name: &'a str,
    /// Package directory (inside the data directory).
    pub package_dir: &'a Path,
    /// Installed PWA build, when present.
    pub web_root: Option<&'a Path>,
}

/// What setup does to `daemon.toml`.
#[derive(Debug, PartialEq, Eq)]
pub enum ConfigChange {
    /// Already this control.
    Unchanged,
    /// Replace the file with this text.
    Write(String),
}

/// Why the existing config cannot become this control.
#[derive(Debug, PartialEq, Eq)]
pub struct Conflict(pub String);

/// Turn the installer's worker config (or no config) into this control's.
pub fn control_config(
    existing: Option<&str>,
    inputs: &ControlInputs<'_>,
) -> Result<ConfigChange, Conflict> {
    let text = existing.unwrap_or_default();
    let value: toml::Value = toml::from_str(text)
        .map_err(|error| Conflict(format!("it is not valid TOML ({error})")))?;
    let control = value.get("control");
    if control
        .and_then(|control| control.get("enabled"))
        .and_then(toml::Value::as_bool)
        == Some(true)
    {
        let url = control
            .and_then(|control| control.get("public_url"))
            .and_then(toml::Value::as_str)
            .unwrap_or_default();
        return if url.trim_end_matches('/') == inputs.origin {
            Ok(ConfigChange::Unchanged)
        } else {
            Err(Conflict(format!(
                "it already makes this Machine the control for {url}"
            )))
        };
    }
    for table in ["control", "packages"] {
        if value.get(table).is_some() {
            return Err(Conflict(format!("it already has a [{table}] table")));
        }
    }
    for key in ["allowed_origins", "behind_tailscale_serve"] {
        if value.get(key).is_some() {
            return Err(Conflict(format!("it already sets {key}")));
        }
    }
    if value.get("development").and_then(toml::Value::as_bool) == Some(true) {
        return Err(Conflict("it is a development config".into()));
    }
    if let Some(bind) = value.get("bind") {
        let ip = bind
            .as_str()
            .and_then(|bind| bind.parse::<std::net::SocketAddr>().ok())
            .map(|bind| bind.ip());
        let ours = ip.is_some_and(|ip| {
            ip.is_loopback() || matches!(ip, std::net::IpAddr::V4(v4) if v4.octets()[0] == 100)
        });
        if !ours {
            return Err(Conflict(format!(
                "its bind {bind} is not a Sentinal default"
            )));
        }
    }

    let quote = |text: &str| toml::Value::String(text.to_string()).to_string();
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let first_table = lines
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let bind_line = format!("bind = {}", quote(API_BIND));
    match lines[..first_table]
        .iter()
        .position(|line| is_key(line, "bind"))
    {
        Some(index) => lines[index] = bind_line,
        None => lines.insert(0, bind_line),
    }
    let first_table = lines
        .iter()
        .position(|line| line.trim_start().starts_with('['))
        .unwrap_or(lines.len());
    let top = [
        "# Added by `sentinal setup`: this Machine is the control, published only".to_string(),
        "# through Tailscale Serve.".to_string(),
        "behind_tailscale_serve = true".to_string(),
        format!("allowed_origins = [{}]", quote(inputs.origin)),
    ];
    lines.splice(first_table..first_table, top);
    if lines.last().is_some_and(|line| !line.trim().is_empty()) {
        lines.push(String::new());
    }
    lines.push("[control]".into());
    lines.push("enabled = true".into());
    lines.push(format!("public_url = {}", quote(inputs.origin)));
    lines.push(format!("display_name = {}", quote(inputs.display_name)));
    lines.push(String::new());
    lines.push("[packages]".into());
    lines.push("enabled = true".into());
    lines.push(format!("listen = {}", quote(PACKAGE_BIND)));
    lines.push(format!(
        "directory = {}",
        quote(&inputs.package_dir.display().to_string())
    ));
    lines.push(format!("public_base_url = {}", quote(inputs.origin)));
    if let Some(web_root) = inputs.web_root {
        lines.push(format!(
            "web_root = {}",
            quote(&web_root.display().to_string())
        ));
    }
    let mut result = lines.join("\n");
    result.push('\n');
    toml::from_str::<toml::Value>(&result)
        .map_err(|error| Conflict(format!("the edited file would be invalid ({error})")))?;
    Ok(ConfigChange::Write(result))
}

fn is_key(line: &str, key: &str) -> bool {
    line.trim_start()
        .strip_prefix(key)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}
