//! Read-only checks for one local worker installation.

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
        let _ = writeln!(out, "Pairing: UNPAIRED");
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
    let _ = writeln!(out, "Pairing: UNPAIRED");

    if [&binaries, &config, &tailscale, &service]
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

fn check_tailscale() -> Result<String, String> {
    let status = Command::new("tailscale")
        .arg("status")
        .output()
        .map_err(|error| format!("cannot run tailscale status: {error}"))?;
    if !status.status.success() {
        return Err(format!("tailscale status exited with {}", status.status));
    }
    let output = Command::new("tailscale")
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
