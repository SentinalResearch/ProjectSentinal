//! Read-only checks for one local installation (worker or control).

use std::{
    io::Write,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

/// Inspect binaries, config, Tailscale, and the native user service.
pub fn run(prefix: Option<PathBuf>, config_dir: Option<PathBuf>, out: &mut dyn Write) -> ExitCode {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        let _ = writeln!(out, "Binaries: ERROR (HOME is not set)");
        let _ = writeln!(out, "Config: ERROR (HOME is not set)");
        let _ = writeln!(out, "Tailscale: ERROR (HOME is not set)");
        let _ = writeln!(out, "Service: ERROR (HOME is not set)");
        let _ = writeln!(out, "Pairing: NOT CONNECTED");
        return ExitCode::FAILURE;
    };
    let prefix = prefix.unwrap_or_else(|| home.join(".local"));
    let config_dir = config_dir.unwrap_or_else(|| home.join(".config/terminal-agent"));

    let binaries = check_binaries(&prefix);
    report(out, "Binaries", &binaries);
    let config = check_config(&config_dir.join("daemon.toml"));
    report(out, "Config", &config);
    let tailscale = check_tailscale();
    report(out, "Tailscale", &tailscale);
    let service = check_service();
    report(out, "Service", &service);

    let settings = read_settings(&config_dir.join("daemon.toml"));
    let release = read_release(&prefix.join("share/sentinal/release.json"));
    let _ = writeln!(out, "Version: {}", describe_version(release.as_ref()));
    let _ = writeln!(out, "Role: {}", describe_role(settings.as_ref()));
    let link_file = settings
        .as_ref()
        .and_then(|settings| settings.link_file.clone())
        .unwrap_or_else(|| config_dir.join("link.json"));
    let pairing = check_pairing(settings.as_ref(), &link_file);
    let _ = match &pairing {
        Ok(detail) => writeln!(out, "Pairing: {detail}"),
        Err(error) => writeln!(out, "Pairing: ERROR ({error})"),
    };
    let _ = writeln!(
        out,
        "Updates: {}",
        describe_updates(settings.as_ref(), release.as_ref())
    );
    let _ = writeln!(
        out,
        "Storage: {}",
        crate::doctor_storage::describe(&config_dir.join("daemon.toml"), &home)
    );

    if [&binaries, &config, &tailscale, &service, &pairing]
        .iter()
        .all(|result| result.is_ok())
    {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn report(out: &mut dyn Write, name: &str, result: &Result<String, String>) {
    let _ = match result {
        Ok(detail) if detail.is_empty() => writeln!(out, "{name}: OK"),
        Ok(detail) => writeln!(out, "{name}: OK ({detail})"),
        Err(error) => writeln!(out, "{name}: ERROR ({error})"),
    };
}

fn check_binaries(prefix: &Path) -> Result<String, String> {
    for name in ["sentinal", "sentinald"] {
        let path = prefix.join("bin").join(name);
        let metadata =
            std::fs::metadata(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        if !metadata.is_file() || !is_executable(&metadata) {
            return Err(format!("{} is not an executable file", path.display()));
        }
    }
    Ok(prefix.join("bin").display().to_string())
}

fn check_config(path: &Path) -> Result<String, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("{} is invalid TOML: {error}", path.display()))?;
    let bind_text = value
        .get("bind")
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("{} has no string bind", path.display()))?;
    let bind: SocketAddr = bind_text
        .parse()
        .map_err(|error| format!("bind '{bind_text}' is invalid: {error}"))?;
    if !is_tailscale_or_loopback(bind.ip()) {
        return Err(format!("bind '{}' is not loopback or Tailscale", bind.ip()));
    }
    Ok(bind.to_string())
}

/// Role and link location from `daemon.toml`; never any credential.
struct Settings {
    control_url: Option<String>,
    link_file: Option<PathBuf>,
}

fn read_settings(path: &Path) -> Option<Settings> {
    let value: toml::Value = toml::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let control = value.get("control");
    let enabled = control
        .and_then(|control| control.get("enabled"))
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let control_url = enabled.then(|| {
        control
            .and_then(|control| control.get("public_url"))
            .and_then(toml::Value::as_str)
            .unwrap_or("no public_url")
            .to_string()
    });
    let link_file = value
        .get("link_file")
        .and_then(toml::Value::as_str)
        .map(PathBuf::from);
    Some(Settings {
        control_url,
        link_file,
    })
}

/// Installed release metadata (`release.json`).
struct Release {
    version: String,
    target: String,
    control_url: Option<String>,
    channel: String,
}

fn read_release(path: &Path) -> Option<Release> {
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
    Some(Release {
        version: text("version")?.to_string(),
        target: text("target").unwrap_or("unknown target").to_string(),
        control_url: text("control_url").map(str::to_string),
        channel: text("channel").unwrap_or("stable").to_string(),
    })
}

fn describe_version(release: Option<&Release>) -> String {
    match release {
        Some(release) => format!("{} ({})", release.version, release.target),
        None => format!(
            "{} (no installed release metadata)",
            crate::version::VERSION.as_str()
        ),
    }
}

fn describe_role(settings: Option<&Settings>) -> String {
    match settings {
        Some(Settings {
            control_url: Some(url),
            ..
        }) => format!("CONTROL ({url})"),
        Some(_) => "WORKER".into(),
        None => "UNKNOWN (daemon.toml is missing or invalid)".into(),
    }
}

/// The worker's link state, read from its link file. Only the state, control
/// URL, and display name are read; credentials are never parsed into output.
fn check_pairing(settings: Option<&Settings>, link_file: &Path) -> Result<String, String> {
    if settings.is_some_and(|settings| settings.control_url.is_some()) {
        return Ok("not needed on the control".into());
    }
    let text = match std::fs::read_to_string(link_file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(
                "NOT CONNECTED (run `sentinal connect <control URL>` with a code from the control)"
                    .into(),
            );
        }
        Err(error) => {
            return Err(format!(
                "cannot read {}: {}",
                link_file.display(),
                error.kind()
            ))
        }
    };
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|_| format!("{} is not a valid link file", link_file.display()))?;
    let field = |key: &str| value.get(key).and_then(serde_json::Value::as_str);
    let control = field("control_url").unwrap_or("an unknown control");
    match field("state") {
        Some("ACTIVE") => Ok(match field("display_name") {
            Some(name) => format!("CONNECTED to {control} as {name}"),
            None => format!("CONNECTED to {control}"),
        }),
        Some("REVOKED") => Ok(format!(
            "REVOKED by {control} (reconnect with a new code from the control)"
        )),
        _ => Err(format!("{} has an unknown link state", link_file.display())),
    }
}

fn describe_updates(settings: Option<&Settings>, release: Option<&Release>) -> String {
    if settings.is_some_and(|settings| settings.control_url.is_some()) {
        return "just receive-release (this Machine is the control and publishes releases)".into();
    }
    match release.and_then(|release| Some((release.control_url.as_deref()?, &release.channel))) {
        Some((url, channel)) => format!("{url} ({channel})"),
        None => {
            "none (installed from source; reinstall from a control's install.sh to enable updates)"
                .into()
        }
    }
}

/// `SENTINAL_TAILSCALE_CLI` when set; otherwise `tailscale` on PATH, then the
/// CLI inside the macOS app bundle (the App Store/standalone app has no PATH entry).
fn tailscale_candidates() -> Vec<PathBuf> {
    if let Some(explicit) =
        std::env::var_os("SENTINAL_TAILSCALE_CLI").filter(|value| !value.is_empty())
    {
        return vec![PathBuf::from(explicit)];
    }
    let mut candidates = vec![PathBuf::from("tailscale")];
    if cfg!(target_os = "macos") {
        candidates.push(PathBuf::from(MACOS_TAILSCALE_CLI));
    }
    candidates
}

const MACOS_TAILSCALE_CLI: &str = "/Applications/Tailscale.app/Contents/MacOS/Tailscale";

fn check_tailscale() -> Result<String, String> {
    let mut last_error = String::from("Tailscale CLI not found");
    for candidate in tailscale_candidates() {
        match check_tailscale_with(&candidate) {
            Ok(ip) => return Ok(ip),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

fn check_tailscale_with(tailscale: &Path) -> Result<String, String> {
    let status = Command::new(tailscale)
        .arg("status")
        .output()
        .map_err(|error| format!("cannot run tailscale status: {error}"))?;
    if !status.status.success() {
        return Err(format!("tailscale status exited with {}", status.status));
    }
    let output = Command::new(tailscale)
        .args(["ip", "-4"])
        .output()
        .map_err(|error| format!("cannot run tailscale ip -4: {error}"))?;
    if !output.status.success() {
        return Err(format!("tailscale ip -4 exited with {}", output.status));
    }
    let address = String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string();
    let ip: Ipv4Addr = address
        .parse()
        .map_err(|error| format!("Tailscale returned invalid IPv4 '{address}': {error}"))?;
    if !is_tailscale_ipv4(ip) {
        return Err(format!("Tailscale IPv4 '{ip}' is outside 100.64.0.0/10"));
    }
    Ok(ip.to_string())
}

fn check_service() -> Result<String, String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("launchctl");
        command.args([
            "print",
            &format!("gui/{}/dev.sentinal.sentinald", current_uid()),
        ]);
        command
    };
    #[cfg(not(target_os = "macos"))]
    let mut command = {
        let mut command = Command::new("systemctl");
        command.args(["--user", "is-active", "sentinald.service"]);
        command
    };
    let status = command
        .output()
        .map_err(|error| format!("cannot inspect user service: {error}"))?;
    if status.status.success() {
        Ok("sentinald".into())
    } else {
        Err(format!("service inspection exited with {}", status.status))
    }
}

fn is_tailscale_or_loopback(ip: IpAddr) -> bool {
    ip.is_loopback()
        || matches!(ip, IpAddr::V4(address) if is_tailscale_ipv4(address))
        || matches!(ip, IpAddr::V6(address) if address.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0])
}

fn is_tailscale_ipv4(address: Ipv4Addr) -> bool {
    u32::from(address) & 0xffc0_0000 == 0x6440_0000
}

#[cfg(unix)]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(metadata: &std::fs::Metadata) -> bool {
    metadata.is_file()
}

#[cfg(target_os = "macos")]
fn current_uid() -> u32 {
    Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0)
}
