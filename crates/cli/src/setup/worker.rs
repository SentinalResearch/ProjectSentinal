//! The WORKER role: list the controls discovery found (never picking one on
//! the owner's behalf), or take a typed URL, then hand it to pairing.

use super::{exit, Io, Outcome, SetupOptions, System};

pub(super) fn worker(
    options: &SetupOptions,
    skip_discovery: bool,
    system: &mut dyn System,
    io: &mut Io<'_>,
) -> Outcome {
    if let Err(outcome) = follow_tailscale_address(options, system, io) {
        return outcome;
    }
    if let Some((url, id)) = system.current_control() {
        return move_worker(options, &url, &id, system, io);
    }
    let url = match &options.control_url {
        Some(url) => url.clone(),
        None if skip_discovery => match ask_url(io) {
            Some(url) => url,
            None => return io.cancelled(),
        },
        None => match choose(system, io) {
            Ok(Some(Choice::Discovered(found))) if found.pairing.approval => {
                return Outcome::Approve {
                    control_url: found.origin,
                    control_machine_id: found.control_machine_id,
                    control_name: found.display_name,
                    moving: false,
                    name: options.name.clone(),
                }
            }
            Ok(Some(Choice::Discovered(found))) => found.origin,
            Ok(Some(Choice::Typed(url))) => url,
            Ok(None) => return io.cancelled(),
            Err(outcome) => return outcome,
        },
    };
    match protocol::link::validate_control_url(&url, protocol::link::ControlUrlPolicy::Production) {
        Ok(_) => Outcome::Connect {
            control_url: url.trim_end_matches('/').to_string(),
            name: options.name.clone(),
        },
        Err(error) => io.fail(
            exit::CANCELLED,
            &format!(
                "'{url}' is not a control URL ({error}); expected https://<name>.<tailnet>.ts.net"
            ),
        ),
    }
}

fn ask_url(io: &mut Io<'_>) -> Option<String> {
    io.ask("Control URL (https://….ts.net): ")
        .filter(|url| !url.is_empty())
}

/// A connected worker moving to another control on the same Tailnet. It
/// stays connected to the current control until the new one approves it.
fn move_worker(
    options: &SetupOptions,
    current_url: &str,
    current_id: &protocol::MachineId,
    system: &mut dyn System,
    io: &mut Io<'_>,
) -> Outcome {
    let _ = writeln!(
        io.out,
        "This Machine is connected to {current_url}. Pick another control to move it; it stays connected here until the new control approves it."
    );
    let found = match system.discover() {
        Ok(found) => found,
        Err(message) => return io.fail(exit::PREREQUISITE, &message),
    };
    let others: Vec<_> = found
        .controls
        .into_iter()
        .map(|control| control.discovery)
        .filter(|control| &control.control_machine_id != current_id)
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
            &format!("there is no other Sentinal control on your Tailnet to move to; this Machine stays connected to {current_url}"),
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
    let Some(answer) = io.ask(&format!("Move to which control? 1-{}: ", others.len())) else {
        return io.cancelled();
    };
    let Some(target) = answer
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
                "{} runs a release without approval pairing; update it, or run sentinal disconnect and then sentinal setup to use a one-time code",
                target.display_name
            ),
        );
    }
    Outcome::Approve {
        control_url: target.origin.clone(),
        control_machine_id: target.control_machine_id.clone(),
        control_name: target.display_name.clone(),
        moving: true,
        name: options.name.clone(),
    }
}

/// What the owner picked.
enum Choice {
    /// A control discovery found (and validated).
    Discovered(protocol::pairing::DiscoveryResponse),
    /// A URL typed by hand: one-time code.
    Typed(String),
}

/// Discover, list, and let the owner pick; `Ok(None)` is a cancel.
fn choose(system: &mut dyn System, io: &mut Io<'_>) -> Result<Option<Choice>, Outcome> {
    let _ = writeln!(io.out, "Looking for Sentinal controls on your Tailnet…");
    let found = match system.discover() {
        Ok(found) => found,
        Err(message) => return Err(io.fail(exit::PREREQUISITE, &message)),
    };
    let silent = if found.unanswered == 0 {
        String::new()
    } else {
        format!("; {} did not answer", found.unanswered)
    };
    let _ = writeln!(io.out, "({} peers checked{silent})", found.checked);
    for origin in &found.older {
        let _ = writeln!(
            io.out,
            "  older Sentinal at {origin}: choose the last option and use a one-time code"
        );
    }
    if found.controls.is_empty() {
        let _ = writeln!(
            io.out,
            "No Sentinal control found on your Tailnet. Enter its URL to use a one-time code."
        );
        return Ok(ask_url(io).map(Choice::Typed));
    }
    for (index, control) in found.controls.iter().enumerate() {
        let discovery = &control.discovery;
        let short_id: String = discovery.control_machine_id.0.chars().take(8).collect();
        let _ = writeln!(
            io.out,
            "  {}) {}   {}   {} · control {short_id}",
            index + 1,
            discovery.display_name,
            discovery.origin,
            discovery.release
        );
    }
    let other = found.controls.len() + 1;
    let _ = writeln!(io.out, "  {other}) Enter a URL / use a one-time code");
    let Some(answer) = io.ask(&format!("Choose 1-{other}: ")) else {
        return Ok(None);
    };
    match answer.parse::<usize>() {
        Ok(choice) if (1..other).contains(&choice) => Ok(Some(Choice::Discovered(
            found.controls[choice - 1].discovery.clone(),
        ))),
        Ok(choice) if choice == other => Ok(ask_url(io).map(Choice::Typed)),
        _ => Ok(None),
    }
}

/// A worker binds its API to its Tailscale address, which changes when the
/// Machine joins another Tailnet. Move `bind` to the current address (after a
/// preview) so the new control can reach it; restore on a failed restart.
fn follow_tailscale_address(
    options: &SetupOptions,
    system: &mut dyn System,
    io: &mut Io<'_>,
) -> Result<(), Outcome> {
    let path = options.config_dir.join("daemon.toml");
    let Ok(existing) = std::fs::read_to_string(&path) else {
        return Ok(());
    };
    let bind = toml::from_str::<toml::Value>(&existing)
        .ok()
        .and_then(|value| {
            value
                .get("bind")?
                .as_str()?
                .parse::<std::net::SocketAddr>()
                .ok()
        });
    let Some(std::net::SocketAddr::V4(bind)) = bind else {
        return Ok(());
    };
    // Only a Tailscale bind follows Tailscale; loopback and others are left alone.
    if bind.ip().octets()[0] != 100 || !(64..=127).contains(&bind.ip().octets()[1]) {
        return Ok(());
    }
    let Ok(tailscale) = system.tailscale() else {
        return Ok(());
    };
    if tailscale.ipv4 == *bind.ip() {
        return Ok(());
    }
    let new_bind = format!("{}:{}", tailscale.ipv4, bind.port());
    let _ = writeln!(
        io.out,
        "This Machine's Tailscale address changed: bind {bind} → {new_bind} (restart: interrupts live FULL Sessions)."
    );
    if !options.yes {
        match io
            .ask("Update the bind and restart sentinald? [y/N]: ")
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("y" | "yes") => {}
            _ => return Err(io.cancelled()),
        }
    }
    let text = super::config::worker_config(&existing, &new_bind)
        .map_err(|conflict| io.fail(exit::CONFLICT, &conflict.0))?;
    let backup = options.config_dir.join("daemon.toml.before-rebind");
    super::control::write_config(&path, &backup, Some(&existing), &text)
        .map_err(|message| io.fail(exit::FAILED, &message))?;
    let restarted = system.restart_service();
    if restarted.is_err() || !system.healthy(&format!("http://{new_bind}/api/v1/health")) {
        let _ = std::fs::write(&path, &existing);
        let _ = system.restart_service();
        return Err(io.fail(exit::FAILED, "sentinald did not start on the new address; the previous config was restored. Run sentinal doctor"));
    }
    let _ = writeln!(io.out, "  ✓ bind {new_bind}");
    Ok(())
}
