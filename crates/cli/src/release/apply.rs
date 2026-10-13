//! Update or roll back the control with the bundled, signature-checking
//! release tools, then require the restarted daemon to pass health checks.

use super::{channel_name, github, state, Context, ReleaseError, Version};
use crate::release_args::ReleaseChannel;
use std::{
    io::{BufRead, IsTerminal, Write},
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant},
};

/// One `sentinal release update` request.
pub(crate) struct UpdateRequest {
    pub(crate) version: Option<String>,
    pub(crate) yes: bool,
    pub(crate) skip_provenance: bool,
    pub(crate) health_timeout: u64,
    pub(crate) channel: ReleaseChannel,
}

const RESTART_WARNING: &str =
    "The dashboard reconnects in about 30 s, and live FULL Sessions on this control are \
interrupted (until Reliability R1). Workers keep running and reconnect on their own.";

/// Publish and install a newer signed release, with automatic rollback.
pub(crate) fn update(
    context: &Context,
    request: UpdateRequest,
    out: &mut dyn Write,
) -> Result<ExitCode, ReleaseError> {
    let target = match &request.version {
        Some(text) => {
            Version::parse(text).ok_or_else(|| ReleaseError::InvalidVersion(text.clone()))?
        }
        None => {
            let outcome = github::check(context, request.channel)?;
            state::record_check(context, request.channel, &outcome)?;
            match outcome.available {
                Some(candidate) => candidate.version,
                None => {
                    github::report(context, request.channel, &outcome, false, out);
                    return Ok(ExitCode::SUCCESS);
                }
            }
        }
    };
    if target <= context.current {
        return Err(ReleaseError::Refused(format!(
            "this control already runs {}; to go back to {target}, use `sentinal release rollback {target}`",
            context.current
        )));
    }
    confirm(
        request.yes,
        &format!(
            "Update this control {} → {target} ({})? {RESTART_WARNING}",
            context.current,
            channel_name(request.channel)
        ),
        out,
    )?;

    let mut receive = Command::new(context.tool("receive-release.sh")?);
    receive
        .args(["--version", &target.to_string(), "--package-dir"])
        .arg(&context.package_dir)
        .arg("--allowed-signers")
        .arg(&context.allowed_signers)
        .args(["--repo", &context.repo]);
    if request.skip_provenance {
        receive.arg("--skip-provenance");
    }
    if !run_tool(receive) {
        state::record_result(
            context,
            "FAILED",
            target,
            Some("publishing or installing the release failed".into()),
        )?;
        return Err(ReleaseError::Refused(format!(
            "updating to {target} failed; this control kept {}",
            context.current
        )));
    }

    if healthy(&context.bind, request.health_timeout) {
        state::record_result(context, "UPDATED", target, None)?;
        let _ = writeln!(out, "Control updated to {target}");
        return Ok(ExitCode::SUCCESS);
    }

    // The new daemon did not come back: restore the previous release.
    let reason = "the restarted daemon did not pass its health check".to_string();
    let previous = context.current;
    if !context
        .package_dir
        .join(format!("releases/{previous}.json"))
        .is_file()
    {
        state::record_result(
            context,
            "FAILED",
            target,
            Some(format!("{reason}; {previous} is not published here")),
        )?;
        return Err(ReleaseError::Refused(format!(
            "{target} is installed but {reason}, and {previous} is not published on this control to roll back to"
        )));
    }
    let restored = restore(context, previous) && healthy(&context.bind, request.health_timeout);
    state::record_result(context, "ROLLED_BACK", target, Some(reason.clone()))?;
    let _ = writeln!(
        out,
        "Rolled back to {previous}: {reason}{}",
        if restored {
            ""
        } else {
            " (the restored daemon is not healthy either; check `sentinal doctor`)"
        }
    );
    Ok(ExitCode::FAILURE)
}

/// Return the control and its channel to an older published release.
pub(crate) fn rollback(
    context: &Context,
    version: &str,
    yes: bool,
    health_timeout: u64,
    out: &mut dyn Write,
) -> Result<ExitCode, ReleaseError> {
    let target =
        Version::parse(version).ok_or_else(|| ReleaseError::InvalidVersion(version.into()))?;
    if !context
        .package_dir
        .join(format!("releases/{target}.json"))
        .is_file()
    {
        return Err(ReleaseError::Refused(format!(
            "release {target} is not published in this control's package directory"
        )));
    }
    confirm(
        yes,
        &format!(
            "Roll this control and its channel back from {} to {target}? {RESTART_WARNING}",
            context.current
        ),
        out,
    )?;
    if !restore(context, target) {
        state::record_result(context, "FAILED", target, Some("rollback failed".into()))?;
        return Err(ReleaseError::Refused(format!(
            "rolling back to {target} failed"
        )));
    }
    if !healthy(&context.bind, health_timeout) {
        state::record_result(
            context,
            "FAILED",
            target,
            Some("the restored daemon is not healthy".into()),
        )?;
        return Err(ReleaseError::Refused(format!(
            "rolled back to {target}, but the daemon is not healthy; check `sentinal doctor`"
        )));
    }
    state::record_result(
        context,
        "ROLLED_BACK",
        target,
        Some("explicit rollback".into()),
    )?;
    let _ = writeln!(out, "Control rolled back to {target}");
    Ok(ExitCode::SUCCESS)
}

/// Move the channel pointer back, then reinstall the signed release.
fn restore(context: &Context, version: Version) -> bool {
    let (Ok(publisher), Ok(applier)) = (
        context.tool("publish-worker-release.sh"),
        context.tool("apply-release.sh"),
    ) else {
        return false;
    };
    let mut pointer = Command::new(publisher);
    pointer
        .args([
            "rollback",
            "--version",
            &version.to_string(),
            "--package-dir",
        ])
        .arg(&context.package_dir);
    let mut apply = Command::new(applier);
    apply
        .args(["--version", &version.to_string(), "--package-dir"])
        .arg(&context.package_dir)
        .arg("--allowed-signers")
        .arg(&context.allowed_signers)
        .arg("--rollback");
    run_tool(pointer) && run_tool(apply)
}

fn run_tool(mut command: Command) -> bool {
    command
        .stdin(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Poll the daemon's health resource until it answers or `timeout_s` passes.
fn healthy(bind: &str, timeout_s: u64) -> bool {
    let deadline = Instant::now() + Duration::from_secs(timeout_s);
    loop {
        let answered = Command::new("curl")
            .args(["--fail", "--silent", "--max-time", "2"])
            .arg(format!("http://{bind}/api/v1/health"))
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|output| {
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("\"ok\":true")
            });
        if answered {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn confirm(yes: bool, question: &str, out: &mut dyn Write) -> Result<(), ReleaseError> {
    if yes {
        return Ok(());
    }
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err(ReleaseError::Refused(
            "confirmation required: re-run in a terminal or with --yes".into(),
        ));
    }
    let _ = write!(out, "{question} [y/N] ");
    let _ = out.flush();
    let mut answer = String::new();
    let _ = stdin.lock().take_line(&mut answer);
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(ReleaseError::Refused(
            "cancelled; nothing was changed".into(),
        ))
    }
}

trait TakeLine {
    fn take_line(&mut self, buffer: &mut String) -> std::io::Result<usize>;
}

impl<T: BufRead> TakeLine for T {
    fn take_line(&mut self, buffer: &mut String) -> std::io::Result<usize> {
        use std::io::Read;
        self.take(64).read_line(buffer)
    }
}

/// Publish the release this control already runs to its own package channel
/// (`sentinal setup`), so workers can install from it. The owner signature is
/// required; GitHub build provenance is checked when `gh` can.
pub(crate) fn publish_installed(
    prefix: &std::path::Path,
    config_dir: &std::path::Path,
    version: &str,
) -> Result<(), ReleaseError> {
    let context = Context::load(&crate::release_args::ReleaseOptions {
        repo: crate::release_args::DEFAULT_RELEASE_REPO.to_string(),
        allowed_signers: None,
        prefix: Some(prefix.to_path_buf()),
        config_dir: Some(config_dir.to_path_buf()),
    })?;
    let version =
        Version::parse(version).ok_or_else(|| ReleaseError::InvalidVersion(version.into()))?;
    // `gh attestation verify` needs gh 2.49+ *and* a signed-in gh; a fresh
    // control usually has neither, and the owner signature is still required.
    let gh = |args: &[&str]| {
        Command::new("gh")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    let provenance = gh(&["attestation", "verify", "--help"]) && gh(&["auth", "status"]);
    let mut receive = Command::new(context.tool("receive-release.sh")?);
    receive
        .args([
            "--version",
            &version.to_string(),
            "--no-self-update",
            "--package-dir",
        ])
        .arg(&context.package_dir)
        .arg("--allowed-signers")
        .arg(&context.allowed_signers)
        .args(["--repo", &context.repo]);
    if !provenance {
        receive.arg("--skip-provenance");
    }
    if run_tool(receive) {
        Ok(())
    } else {
        Err(ReleaseError::Refused(format!(
            "publishing {version} for workers failed; run sentinal setup again"
        )))
    }
}
