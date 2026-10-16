//! A control becoming a worker of another control. Its workers are handed
//! off first (each asks the new control's owner for approval), then the
//! config loses its control role, the service restarts on the Tailscale
//! address, setup's Serve routes go, and this Machine asks for approval too.
//! A failed restart restores the control config; nothing else has changed.

use super::{config::worker_config, exit, Io, Outcome, SetupOptions, System};
use std::fs;

pub(super) fn to_worker(
    options: &SetupOptions,
    existing: &str,
    system: &mut dyn System,
    io: &mut Io<'_>,
) -> Outcome {
    let config_path = options.config_dir.join("daemon.toml");
    let own_origin = toml::from_str::<toml::Value>(existing)
        .ok()
        .and_then(|value| {
            value
                .get("control")?
                .get("public_url")?
                .as_str()
                .map(|url| url.trim_end_matches('/').to_string())
        })
        .unwrap_or_default();
    let tailscale = match system.tailscale() {
        Ok(tailscale) => tailscale,
        Err(message) => return io.fail(exit::PREREQUISITE, &message),
    };
    let _ = writeln!(
        io.out,
        "This Machine is a Sentinal control. Looking for the control it should join…"
    );
    let found = match system.discover() {
        Ok(found) => found,
        Err(message) => return io.fail(exit::PREREQUISITE, &message),
    };
    let others: Vec<_> = found
        .controls
        .into_iter()
        .map(|control| control.discovery)
        .filter(|control| control.origin != own_origin)
        .filter(|control| {
            options
                .control_url
                .as_deref()
                .is_none_or(|url| url.trim_end_matches('/') == control.origin)
        })
        .collect();
    if others.is_empty() {
        return io.fail(
            exit::CONFLICT,
            "there is no other Sentinal control on your Tailnet to join; this Machine stays the control",
        );
    }
    for (index, control) in others.iter().enumerate() {
        let short_id: String = control.control_machine_id.0.chars().take(8).collect();
        let _ = writeln!(
            io.out,
            "  {}) {}   {}   {} · control {short_id}",
            index + 1,
            control.display_name,
            control.origin,
            control.release
        );
    }
    let Some(choice) = io.ask(&format!("Join which control? 1-{}: ", others.len())) else {
        return io.cancelled();
    };
    let Some(target) = choice
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|n| others.get(n))
    else {
        return io.cancelled();
    };
    if !target.pairing.approval {
        return io.fail(
            exit::CONFLICT,
            &format!(
                "{} runs a release without approval pairing; update it first",
                target.display_name
            ),
        );
    }

    let workers = match system.handoff(&target.origin, &target.control_machine_id, true) {
        Ok(workers) => workers,
        Err(message) => {
            return io.fail(
                exit::FAILED,
                &format!("cannot list this control's workers: {message}"),
            )
        }
    };
    let names: Vec<&str> = workers
        .iter()
        .map(|worker| worker.display_name.as_str())
        .collect();
    let _ = writeln!(io.out, "Preview — nothing has changed yet:");
    if workers.is_empty() {
        let _ = writeln!(io.out, "  workers       none are paired to this control");
    } else {
        let _ = writeln!(
            io.out,
            "  workers       {} worker{} ({}) will ask {} to approve them; approve each in its dashboard",
            workers.len(),
            if workers.len() == 1 { "" } else { "s" },
            names.join(", "),
            target.display_name
        );
    }
    let _ = writeln!(io.out, "  config        {} becomes a worker config (control config kept as daemon.toml.control-backup)", config_path.display());
    let _ = writeln!(
        io.out,
        "  daemon        restarts on {}:7420 (restart: interrupts live FULL Sessions)",
        tailscale.ipv4
    );
    let _ = writeln!(
        io.out,
        "  Tailscale     this Machine's / and /api routes are removed"
    );
    let _ = writeln!(
        io.out,
        "  data          kept (Machine ID, history); this dashboard stops serving"
    );
    if !options.yes {
        match io
            .ask(&format!(
                "Hand off and join {}? [y/N]: ",
                target.display_name
            ))
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("y" | "yes") => {}
            _ => return io.cancelled(),
        }
    }

    match system.handoff(&target.origin, &target.control_machine_id, false) {
        Ok(moved) => {
            for worker in moved {
                match worker.message {
                    None if worker.moved => {
                        let _ = writeln!(
                            io.out,
                            "  ✓ {} asked {}",
                            worker.display_name, target.display_name
                        );
                    }
                    message => {
                        let _ = writeln!(
                            io.out,
                            "  ✗ {}: {} (run sentinal setup on it later)",
                            worker.display_name,
                            message.unwrap_or_else(|| "not moved".into())
                        );
                    }
                }
            }
        }
        Err(message) => {
            return io.fail(
                exit::FAILED,
                &format!("the handoff failed before anything changed: {message}"),
            )
        }
    }

    let worker = match worker_config(existing, &format!("{}:7420", tailscale.ipv4)) {
        Ok(text) => text,
        Err(conflict) => {
            return io.fail(
                exit::CONFLICT,
                &format!("{} was not changed: {}", config_path.display(), conflict.0),
            )
        }
    };
    let backup = options.config_dir.join("daemon.toml.control-backup");
    if let Err(message) =
        super::control::write_config(&config_path, &backup, Some(existing), &worker)
    {
        return io.fail(exit::FAILED, &message);
    }
    let restarted = system.restart_service();
    let health = format!("http://{}:7420/api/v1/health", tailscale.ipv4);
    if restarted.is_err() || !system.healthy(&health) {
        let _ = fs::write(&config_path, existing);
        let _ = system.restart_service();
        return io.fail(
            exit::FAILED,
            "the daemon did not start as a worker, so the control config was restored and this Machine is still the control. Run sentinal doctor",
        );
    }
    let _ = writeln!(io.out, "  ✓ config  ✓ service  ✓ health");
    match system.stop_serving() {
        Ok(()) => {
            let _ = writeln!(io.out, "  ✓ Serve routes removed");
        }
        Err(message) => {
            let _ = writeln!(io.out, "  · could not remove the Serve routes ({message}); run: tailscale serve --https=443 off");
        }
    }
    Outcome::Approve {
        control_url: target.origin.clone(),
        control_machine_id: target.control_machine_id.clone(),
        control_name: target.display_name.clone(),
        name: options.name.clone(),
    }
}
