//! The WORKER role: list the controls discovery found (never picking one on
//! the owner's behalf), or take a typed URL, then hand it to pairing.

use super::{exit, Io, Outcome, SetupOptions, System};

pub(super) fn worker(
    options: &SetupOptions,
    skip_discovery: bool,
    system: &mut dyn System,
    io: &mut Io<'_>,
) -> Outcome {
    let url = match &options.control_url {
        Some(url) => url.clone(),
        None if skip_discovery => match ask_url(io) {
            Some(url) => url,
            None => return io.cancelled(),
        },
        None => match choose(system, io) {
            Ok(Some(url)) => url,
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

/// Discover, list, and let the owner pick; `Ok(None)` is a cancel.
fn choose(system: &mut dyn System, io: &mut Io<'_>) -> Result<Option<String>, Outcome> {
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
        return Ok(ask_url(io));
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
        Ok(choice) if (1..other).contains(&choice) => {
            Ok(Some(found.controls[choice - 1].discovery.origin.clone()))
        }
        Ok(choice) if choice == other => Ok(ask_url(io)),
        _ => Ok(None),
    }
}
