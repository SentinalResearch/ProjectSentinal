//! `sentinal doctor --pilot` (4.E §9): read-only checks that a control VM is
//! private and working. It runs `lsof`/`ss`, `tailscale serve status`, and
//! `curl`, and reads the package channel and backup directory; it changes
//! nothing and sends no credential.

use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime},
};

const PORTS: [&str; 2] = ["7420", "7422"];

/// Print the pilot lines; `false` when any check failed.
pub fn run(daemon_toml: &Path, home: &Path, out: &mut dyn Write) -> bool {
    let value: Option<toml::Value> = std::fs::read_to_string(daemon_toml)
        .ok()
        .and_then(|text| toml::from_str(&text).ok());
    let origin = value
        .as_ref()
        .filter(|value| {
            value
                .get("control")
                .and_then(|c| c.get("enabled"))
                .and_then(toml::Value::as_bool)
                == Some(true)
        })
        .and_then(|value| {
            value
                .get("control")?
                .get("public_url")?
                .as_str()
                .map(|url| url.trim_end_matches('/').to_string())
        });
    let Some(origin) = origin else {
        let _ = writeln!(
            out,
            "Pilot: ERROR (not a control; run sentinal setup and choose the control role)"
        );
        return false;
    };
    let package_dir = value.as_ref().and_then(|value| {
        value
            .get("packages")?
            .get("directory")?
            .as_str()
            .map(PathBuf::from)
    });
    let checks = [
        ("Pilot listeners", listeners()),
        ("Pilot Serve", serve(&origin)),
        ("Pilot HTTPS", https(&origin)),
        ("Pilot packages", packages(package_dir.as_deref())),
        ("Pilot backup", backup(&home.join("sentinal-backups"))),
    ];
    let mut ok = true;
    for (name, result) in checks {
        let _ = match &result {
            Ok(detail) => writeln!(out, "{name}: OK ({detail})"),
            Err(problem) => writeln!(out, "{name}: ERROR ({problem})"),
        };
        ok &= result.is_ok();
    }
    ok
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Listening addresses for 7420/7422 must be loopback.
fn listeners() -> Result<String, String> {
    let addresses: Vec<String> =
        if let Some(text) = output("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN"]) {
            text.lines()
                .skip(1)
                .filter_map(|line| line.split_whitespace().rev().nth(1).map(str::to_string))
                .collect()
        } else if let Some(text) = output("ss", &["-ltnH"]) {
            text.lines()
                .filter_map(|line| line.split_whitespace().nth(3).map(str::to_string))
                .collect()
        } else {
            return Err("neither lsof nor ss is available to list listeners".into());
        };
    let mut seen = 0;
    for address in &addresses {
        let Some((host, port)) = address.rsplit_once(':') else {
            continue;
        };
        if !PORTS.contains(&port) {
            continue;
        }
        seen += 1;
        if !matches!(host, "127.0.0.1" | "[::1]" | "::1" | "localhost") {
            return Err(format!("{address} listens beyond loopback"));
        }
    }
    if seen == 0 {
        return Err("nothing listens on 7420 or 7422; is sentinald running?".into());
    }
    Ok("7420 and 7422 on loopback only".into())
}

fn serve(origin: &str) -> Result<String, String> {
    let host = origin.trim_start_matches("https://");
    let text = crate::doctor::tailscale_candidates()
        .iter()
        .find_map(|cli| output(&cli.to_string_lossy(), &["serve", "status", "--json"]))
        .ok_or("cannot read tailscale serve status")?;
    let status: serde_json::Value = if text.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(&text).map_err(|_| "tailscale serve status is not JSON".to_string())?
    };
    let missing = crate::setup::serve_plan(&status, host)?;
    if !missing.is_empty() {
        return Err("a Serve route is missing; run sentinal setup".into());
    }
    Ok("/ and /api on 443; Funnel off".into())
}

fn https(origin: &str) -> Result<String, String> {
    let get = |path: &str| {
        output(
            "curl",
            &[
                "--fail",
                "--silent",
                "--max-time",
                "5",
                "--max-redirs",
                "0",
                &format!("{origin}{path}"),
            ],
        )
    };
    match get("/api/v1/health") {
        Some(body) if body.contains("\"ok\":true") => {}
        _ => return Err(format!("{origin}/api/v1/health did not answer")),
    }
    if get("/").is_none() {
        return Err("the dashboard did not answer".into());
    }
    if get("/install.sh").is_none() {
        return Err("install.sh did not answer; it appears once a release is published (see Pilot packages)".into());
    }
    Ok("health, dashboard, install.sh".into())
}

fn packages(package_dir: Option<&Path>) -> Result<String, String> {
    let dir = package_dir.ok_or("[packages] has no directory")?;
    let text = std::fs::read_to_string(dir.join("channels/stable.json"))
        .map_err(|_| "no release is published to stable; run sentinal setup, which publishes the installed release".to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "stable.json is not JSON".to_string())?;
    let version = value
        .get("release_version")
        .or_else(|| value.get("version"))
        .and_then(serde_json::Value::as_str)
        .ok_or("stable.json names no release")?;
    Ok(format!("stable {version}"))
}

fn backup(dir: &Path) -> Result<String, String> {
    let newest = std::fs::read_dir(dir)
        .map_err(|_| {
            format!(
                "no backups in {}; run sentinal backup create",
                dir.display()
            )
        })?
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tar.gz"))
        .filter_map(|entry| entry.metadata().ok()?.modified().ok())
        .max()
        .ok_or_else(|| {
            format!(
                "no backups in {}; run sentinal backup create",
                dir.display()
            )
        })?;
    let age = SystemTime::now().duration_since(newest).unwrap_or_default();
    let days = age.as_secs() / 86_400;
    if age > Duration::from_secs(7 * 86_400) {
        return Err(format!(
            "the newest backup is {days} days old; run sentinal backup create"
        ));
    }
    Ok(if days == 0 {
        "newest is less than a day old".into()
    } else {
        format!(
            "newest is {days} day{} old",
            if days == 1 { "" } else { "s" }
        )
    })
}
