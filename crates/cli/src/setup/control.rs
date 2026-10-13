//! The CONTROL role: plan the config, Serve routes, and linger; preview them;
//! apply in an order that can be undone; then publish the installed release.

use super::{
    config::{control_config, ConfigChange, ControlInputs, API_BIND, PACKAGE_BIND},
    exit, Io, Outcome, ServeRoute, SetupOptions, System,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

pub(super) fn control(
    options: &SetupOptions,
    existing: Option<&str>,
    system: &mut dyn System,
    io: &mut Io<'_>,
) -> Outcome {
    let config_path = options.config_dir.join("daemon.toml");
    if options.config_dir.join("link.json").exists() {
        return io.fail(
            exit::CONFLICT,
            "this Machine is connected to a control as a worker. Run 'sentinal disconnect' first to make it a control",
        );
    }
    let tailscale = match system.tailscale() {
        Ok(tailscale) => tailscale,
        Err(message) => return io.fail(exit::PREREQUISITE, &message),
    };
    let origin = format!("https://{}", tailscale.dns_name);
    let serve = match system.serve_status() {
        Ok(status) => status,
        Err(message) => {
            return io.fail(
                exit::PREREQUISITE,
                &format!("cannot read Tailscale Serve: {message}"),
            )
        }
    };
    let missing_routes = match serve_plan(&serve, &tailscale.dns_name) {
        Ok(routes) => routes,
        Err(message) => return io.fail(exit::CONFLICT, &message),
    };
    let data_dir = existing
        .and_then(|text| toml::from_str::<toml::Value>(text).ok())
        .and_then(|value| value.get("data_dir")?.as_str().map(PathBuf::from))
        .unwrap_or_else(|| options.home.join(".local/share/terminal-agent"));
    let package_dir = data_dir.join("packages");
    let web_root = options.prefix.join("share/sentinal/web");

    // The name is only asked for when the config will be written.
    let check = control_config(
        existing,
        &ControlInputs {
            origin: &origin,
            display_name: "",
            package_dir: &package_dir,
            web_root: None,
        },
    );
    let change = match check {
        Err(conflict) => {
            return io.fail(
                exit::CONFLICT,
                &format!("{} was not changed: {}", config_path.display(), conflict.0),
            )
        }
        Ok(ConfigChange::Unchanged) => ConfigChange::Unchanged,
        Ok(ConfigChange::Write(_)) => {
            let default_name = format!("{}'s Sentinal", options.user);
            let name = match (&options.name, options.yes) {
                (Some(name), _) => name.clone(),
                (None, true) => default_name,
                (None, false) => match io.ask(&format!("Name for this control [{default_name}]: "))
                {
                    Some(name) if name.is_empty() => default_name,
                    Some(name) => name,
                    None => return io.cancelled(),
                },
            };
            match control_config(
                existing,
                &ControlInputs {
                    origin: &origin,
                    display_name: &name,
                    package_dir: &package_dir,
                    web_root: web_root.is_dir().then_some(web_root.as_path()),
                },
            ) {
                Ok(change) => change,
                Err(conflict) => return io.fail(exit::CONFLICT, &conflict.0),
            }
        }
    };
    let linger_missing = system.linger(&options.user) == Some(false);
    let published = package_dir
        .join(format!("releases/{}.json", options.version))
        .is_file();

    if change == ConfigChange::Unchanged && missing_routes.is_empty() && !linger_missing {
        let _ = writeln!(io.out, "Already a Sentinal control at {origin}.");
        let local = system.healthy(&format!("http://{API_BIND}/api/v1/health"));
        let public = local && system.healthy(&format!("{origin}/api/v1/health"));
        if !public {
            return io.fail(
                exit::FAILED,
                "the daemon does not answer its health check; run sentinal doctor",
            );
        }
        if !published {
            if let Outcome::Exit(code) = publish(options, system, io, &origin) {
                if code != exit::OK {
                    return Outcome::Exit(code);
                }
            }
        }
        let had_password = system.password_needed() != Some(true);
        let password = super::password::ensure(options, token_file(existing, options), system, io);
        if published && had_password {
            let _ = writeln!(io.out, "Nothing to do.");
        } else if password {
            let _ = writeln!(io.out, "Done. Open {origin} and sign in.");
        }
        return Outcome::Exit(exit::OK);
    }

    preview(
        options,
        io,
        &change,
        &missing_routes,
        linger_missing,
        &config_path,
        &data_dir,
        &origin,
    );
    if !options.yes {
        match io
            .ask("Apply? [y/N]: ")
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("y" | "yes") => {}
            _ => return io.cancelled(),
        }
    }

    if linger_missing {
        if let Err(message) = system.enable_linger(&options.user) {
            return io.fail(exit::FAILED, &format!("nothing was changed. {message}"));
        }
        let _ = writeln!(io.out, "  ✓ linger");
    }
    if let ConfigChange::Write(text) = &change {
        let backup = options.config_dir.join("daemon.toml.before-setup");
        if let Err(message) = write_config(&config_path, &backup, existing, text) {
            return io.fail(exit::FAILED, &message);
        }
        let _ = writeln!(io.out, "  ✓ config");
        let restarted = system.restart_service();
        let healthy =
            restarted.is_ok() && system.healthy(&format!("http://{API_BIND}/api/v1/health"));
        if !healthy {
            let reason = restarted
                .err()
                .unwrap_or_else(|| "it did not answer its health check".into());
            let restored = restore_config(&config_path, &backup, existing);
            let _ = system.restart_service();
            return io.fail(
                exit::FAILED,
                &format!(
                    "the daemon did not start with the control config ({reason}); {} was restored. Run sentinal doctor",
                    if restored.is_ok() { config_path.display().to_string() } else { "nothing".into() }
                ),
            );
        }
        let _ = writeln!(io.out, "  ✓ service  ✓ health");
    }
    for route in &missing_routes {
        if let Err(message) = system.serve(*route) {
            return io.fail(exit::FAILED, &message);
        }
    }
    if !missing_routes.is_empty() {
        let _ = writeln!(io.out, "  ✓ Serve");
    }
    if !system.healthy(&format!("{origin}/api/v1/health")) {
        return io.fail(
            exit::FAILED,
            &format!("{origin}/api/v1/health does not answer; check 'tailscale serve status', then sentinal setup"),
        );
    }
    if !published {
        if let Outcome::Exit(code) = publish(options, system, io, &origin) {
            if code != exit::OK {
                return Outcome::Exit(code);
            }
        }
    }
    if super::password::ensure(options, token_file(existing, options), system, io) {
        let _ = writeln!(io.out, "Done. Open {origin} and sign in.");
    } else {
        let _ = writeln!(
            io.out,
            "Done. Open {origin} once the owner password is set."
        );
    }
    Outcome::Exit(exit::OK)
}

/// The daemon's Machine token file (`token_file`, or the default).
fn token_file(existing: Option<&str>, options: &SetupOptions) -> PathBuf {
    existing
        .and_then(|text| toml::from_str::<toml::Value>(text).ok())
        .and_then(|value| value.get("token_file")?.as_str().map(PathBuf::from))
        .unwrap_or_else(|| options.config_dir.join("token"))
}

fn publish(
    options: &SetupOptions,
    system: &mut dyn System,
    io: &mut Io<'_>,
    origin: &str,
) -> Outcome {
    match system.publish(&options.version, &options.config_dir) {
        Ok(()) => {
            let _ = writeln!(
                io.out,
                "  ✓ release {} published for workers ({origin}/install.sh)",
                options.version
            );
            Outcome::Exit(exit::OK)
        }
        Err(message) => io.fail(exit::FAILED, &format!("the control works, but {message}")),
    }
}

/// Routes still to add on HTTPS 443, or why Serve cannot be used.
pub(crate) fn serve_plan(
    status: &serde_json::Value,
    dns_name: &str,
) -> Result<Vec<ServeRoute>, String> {
    let host = format!("{dns_name}:443");
    if status
        .get("AllowFunnel")
        .and_then(|funnel| funnel.get(&host))
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Err(format!(
            "Tailscale Funnel is on for {host}, which would publish Sentinal to the internet. Turn it off ('tailscale funnel reset'), then sentinal setup"
        ));
    }
    if status
        .get("TCP")
        .and_then(|tcp| tcp.get("443"))
        .is_some_and(|port| port.get("HTTPS").and_then(serde_json::Value::as_bool) != Some(true))
    {
        return Err(
            "port 443 is forwarded as raw TCP by Tailscale Serve; free it, then sentinal setup"
                .into(),
        );
    }
    let handlers = status
        .get("Web")
        .and_then(|web| web.get(&host))
        .and_then(|web| web.get("Handlers"));
    let mut missing = Vec::new();
    for route in [ServeRoute::Root, ServeRoute::Api] {
        let handler = handlers.and_then(|handlers| handlers.get(route.path()));
        match handler.and_then(|handler| handler.get("Proxy")).and_then(serde_json::Value::as_str) {
            None if handler.is_none() => missing.push(route),
            Some(target) if target == route.target() => {}
            other => {
                return Err(format!(
                    "Tailscale Serve already sends https://{dns_name}{} to {}; setup never replaces another route. Remove it ('tailscale serve status'), then sentinal setup",
                    route.path(),
                    other.unwrap_or("something else")
                ))
            }
        }
    }
    Ok(missing)
}

#[allow(clippy::too_many_arguments)]
fn preview(
    options: &SetupOptions,
    io: &mut Io<'_>,
    change: &ConfigChange,
    routes: &[ServeRoute],
    linger_missing: bool,
    config_path: &Path,
    data_dir: &Path,
    origin: &str,
) {
    let out = &mut io.out;
    let _ = writeln!(out, "Preview — nothing has changed yet:");
    let _ = writeln!(
        out,
        "  daemon        {API_BIND} (API)   {PACKAGE_BIND} (PWA + packages)   never 0.0.0.0"
    );
    if matches!(change, ConfigChange::Write(_)) {
        let _ = writeln!(
            out,
            "  config        {}   (previous copy kept as daemon.toml.before-setup)",
            config_path.display()
        );
        let _ = writeln!(
            out,
            "  service       restart sentinald (restart: interrupts live FULL Sessions)"
        );
    }
    let _ = writeln!(
        out,
        "  data          {}   backup boundary: config + data dirs",
        data_dir.display()
    );
    if linger_missing {
        let _ = writeln!(
            out,
            "  keep running  sudo loginctl enable-linger {}   (asks for your password once)",
            options.user
        );
    }
    for route in routes {
        let _ = writeln!(
            out,
            "  Tailscale     {origin}{} → {}",
            route.path().trim_end_matches('/'),
            route.target()
        );
    }
    let _ = writeln!(out, "                Funnel: never");
    let _ = writeln!(
        out,
        "  dashboard     {origin}  (set the owner password there)"
    );
}

fn write_config(
    path: &Path,
    backup: &Path,
    existing: Option<&str>,
    text: &str,
) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    let describe = |error: std::io::Error| format!("could not write {}: {error}", path.display());
    if let Some(existing) = existing {
        write_private(backup, existing).map_err(describe)?;
    }
    let temporary = path.with_extension("toml.setup-tmp");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(describe)?;
    file.write_all(text.as_bytes()).map_err(describe)?;
    file.sync_all().map_err(describe)?;
    fs::rename(&temporary, path).map_err(describe)
}

fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(text.as_bytes())
}

fn restore_config(path: &Path, backup: &Path, existing: Option<&str>) -> std::io::Result<()> {
    match existing {
        Some(text) => {
            write_private(path, text)?;
            fs::remove_file(backup)
        }
        None => fs::remove_file(path),
    }
}
