//! `sentinal complete` (Milestone 5.A): ranked completions for the line being
//! typed, from this Machine's daemon over its private local socket. The
//! shell widgets in `shell/external/` call it on Ctrl-Space.

use protocol::{complete::Completion, LocalRequest, LocalResponse};
use std::{io::Write, path::PathBuf, process::ExitCode, time::Duration};

/// Environment variable carrying the typed text (never argv, which `ps` shows).
pub const PREFIX_ENV: &str = "SENTINAL_COMPLETE_PREFIX";
/// A key press must never hang the shell.
const DEADLINE: Duration = Duration::from_millis(1_500);

/// The request for `prefix` (else `$SENTINAL_COMPLETE_PREFIX`) in `cwd`.
pub fn request(
    prefix: Option<String>,
    cwd: Option<PathBuf>,
    limit: u32,
    include_failed: bool,
) -> LocalRequest {
    let prefix = prefix
        .or_else(|| std::env::var(PREFIX_ENV).ok())
        .unwrap_or_default();
    let cwd = cwd
        .or_else(|| std::env::current_dir().ok())
        .map(|cwd| cwd.display().to_string());
    LocalRequest::Complete {
        session_id: None,
        prefix,
        cwd,
        include_failed,
        limit,
    }
}

/// Lines to print: one completion per line, or a JSON array.
pub fn render(completions: &[Completion], json: bool) -> String {
    if json {
        return serde_json::to_string(completions).unwrap_or_else(|_| "[]".into()) + "\n";
    }
    completions
        .iter()
        .map(|completion| format!("{}\n", completion.text))
        .collect()
}

/// Ask the daemon and print what it offers. An empty answer still succeeds.
pub fn run(
    request: LocalRequest,
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    match crate::setup::local_socket_within(request, DEADLINE) {
        Ok(LocalResponse::Completions { completions }) => {
            let _ = out.write_all(render(&completions, json).as_bytes());
            ExitCode::SUCCESS
        }
        Ok(LocalResponse::Error { message, .. }) => {
            let _ = writeln!(err, "sentinal complete: {message}");
            ExitCode::FAILURE
        }
        Ok(_) => {
            let _ = writeln!(err, "sentinal complete: unexpected reply from sentinald");
            ExitCode::FAILURE
        }
        Err(message) => {
            let _ = writeln!(err, "sentinal complete: {message}");
            ExitCode::FAILURE
        }
    }
}
