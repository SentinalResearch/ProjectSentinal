//! A control becoming a worker of another control. Its workers are handed
//! off first (each asks the new control's owner for approval), then the
//! config loses its control role, the service restarts on the Tailscale
//! address, setup's Serve routes go, and this Machine asks for approval too.
//! A failed restart restores the control config; nothing else has changed.

use super::{config::worker_config, exit, Io, Outcome, SetupOptions, System};
use protocol::pairing::WorkerHandoff;
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
    let none = others.len() + 1;
    let _ = writeln!(
        io.out,
        "  {none}) None yet: become a worker that is not connected to any control"
    );
    let Some(choice) = io.ask(&format!("Join which control? 1-{none}: ")) else {
        return io.cancelled();
    };
    let target = match choice.parse::<usize>() {
        Ok(n) if n == none => None,
        Ok(n) => match n.checked_sub(1).and_then(|n| others.get(n)) {
            Some(target) => Some(target),
            None => return io.cancelled(),
        },
        Err(_) => return io.cancelled(),
    };
    if let Some(target) = target.filter(|target| !target.pairing.approval) {
        return io.fail(
            exit::CONFLICT,
            &format!(
                "{} runs a release without approval pairing; update it first",
                target.display_name
            ),
        );
    }
    let target_ref = target.map(|target| (target.origin.as_str(), &target.control_machine_id));

    let workers = match system.handoff(None, true) {
        Ok(workers) => workers,
        Err(message) => {
            return io.fail(
                exit::FAILED,
                &format!("cannot list this control's workers: {message}"),
            )
        }
    };
    let _ = writeln!(io.out, "Preview — nothing has changed yet:");
    preview_workers(
        io,
        &workers,
        target.map(|target| target.display_name.as_str()),
    );
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
        let question = match target {
            Some(target) => format!("Hand off and join {}? [y/N]: ", target.display_name),
            None => "Stop being a control and become an unconnected worker? [y/N]: ".into(),
        };
        match io
            .ask(&question)
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("y" | "yes") => {}
            _ => return io.cancelled(),
        }
    }

    if let Some(target) = target {
        match system.handoff(target_ref, false) {
            Ok(moved) => report_moves(io, &moved, &target.display_name),
            Err(message) => {
                return io.fail(
                    exit::FAILED,
                    &format!("the handoff failed before anything changed: {message}"),
                )
            }
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
    match target {
        Some(target) => Outcome::Approve {
            control_url: target.origin.clone(),
            control_machine_id: target.control_machine_id.clone(),
            control_name: target.display_name.clone(),
            moving: false,
            name: options.name.clone(),
        },
        None => {
            let _ = writeln!(
                io.out,
                "This Machine is now a worker, not connected to any control. When it is on the Tailnet of the control it should join, run sentinal setup."
            );
            Outcome::Exit(exit::OK)
        }
    }
}

fn preview_workers(io: &mut Io<'_>, workers: &[WorkerHandoff], target: Option<&str>) {
    let names: Vec<&str> = workers
        .iter()
        .map(|worker| worker.display_name.as_str())
        .collect();
    let _ = match (workers.len(), target) {
        (0, _) => writeln!(io.out, "  workers       none are paired to this control"),
        (count, Some(target)) => writeln!(
            io.out,
            "  workers       {count} worker{} ({}) will ask {target} to approve them; approve each in its dashboard",
            if count == 1 { "" } else { "s" },
            names.join(", ")
        ),
        (count, None) => writeln!(
            io.out,
            "  workers       {} will lose {} control; run sentinal setup on {} afterwards",
            names.join(", "),
            if count == 1 { "its" } else { "their" },
            if count == 1 { "it" } else { "each" }
        ),
    };
}

fn report_moves(io: &mut Io<'_>, moved: &[WorkerHandoff], target: &str) {
    for worker in moved {
        let _ = match &worker.message {
            None if worker.moved => writeln!(io.out, "  ✓ {} asked {target}", worker.display_name),
            message => writeln!(
                io.out,
                "  ✗ {}: {} (run sentinal setup on it later)",
                worker.display_name,
                message.as_deref().unwrap_or("not moved")
            ),
        };
    }
}
