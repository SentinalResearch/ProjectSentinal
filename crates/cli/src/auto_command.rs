//! `sentinal auto on|off|status` and the hidden `auto enter` used by the rc block.

use crate::{
    args::AutoAction,
    auto_wrap::{
        loader_body, skip_reason, within, AutoEnv, ENTRY_DEADLINE, FALLBACK_EXIT, SIGNAL_VARS,
    },
    client::{terminal_size, DaemonClient},
    hook_command::{binary, rc_env, shown},
    hook_script::{ensure_not_world_writable, HookShell},
    rc_file::{find_block, install_block, uninstall_block, Markers, Placement},
    rc_install::{apply, select_rc, Applied},
    watch::SESSION_ENV,
};
use protocol::{ClientRequest, DaemonMessage, SessionOrigin};
use std::{
    io::{IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Instant,
};

/// Run one auto-wrap operation.
pub async fn run(
    action: AutoAction,
    urls: &[String],
    token_file: Option<PathBuf>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    let result = match action {
        AutoAction::Enter { shell, shell_path } => {
            return ExitCode::from(enter(urls, token_file, &shell, &shell_path).await);
        }
        AutoAction::On { shell, rc } => edit(shell, rc, true, out),
        AutoAction::Off { shell, rc } => edit(shell, rc, false, out),
        AutoAction::Status { shell, rc } => status(shell, rc, out),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(err, "sentinal: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Snapshot the signals for a starting (or current) shell.
pub fn capture_env(shell: Option<HookShell>, interactive: bool) -> AutoEnv {
    AutoEnv {
        vars: SIGNAL_VARS
            .iter()
            .filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| (name.to_string(), value))
            })
            .chain(configured_skip_vars())
            .collect(),
        interactive,
        stdin_tty: std::io::stdin().is_terminal(),
        stdout_tty: std::io::stdout().is_terminal(),
        shell,
    }
}

/// Values of the variables named in `SENTINAL_AUTO_SKIP`.
fn configured_skip_vars() -> Vec<(String, String)> {
    std::env::var("SENTINAL_AUTO_SKIP")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.to_string(), value))
        })
        .collect()
}

async fn enter(urls: &[String], token_file: Option<PathBuf>, shell: &str, shell_path: &str) -> u8 {
    let started = Instant::now();
    let hook_shell = HookShell::from_shell_path(shell);
    if skip_reason(&capture_env(hook_shell, true)).is_some() {
        return FALLBACK_EXIT;
    }
    let Some(token_file) = token_file.or_else(default_token_file) else {
        return FALLBACK_EXIT;
    };
    let cwd = std::env::var("PWD")
        .ok()
        .filter(|pwd| pwd.starts_with('/'))
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|dir| dir.to_string_lossy().into_owned())
        });
    let (cols, rows) = terminal_size().unwrap_or((80, 24));
    let request = ClientRequest::CreateSession {
        shell: Some(shell_path.to_string()).filter(|path| path.starts_with('/')),
        cwd,
        cols,
        rows,
        origin: SessionOrigin::AutoWrap,
        title: None,
        command: None,
    };
    let created = within(ENTRY_DEADLINE, async {
        let mut client = DaemonClient::connect(urls, &token_file).await.ok()?;
        match client.request(request).await.ok()? {
            DaemonMessage::SessionUpdated { session } => Some((client, session)),
            _ => None,
        }
    })
    .await;
    let Some((mut client, session)) = created else {
        return FALLBACK_EXIT;
    };
    if session.origin != SessionOrigin::AutoWrap {
        // An older daemon would not mark the shell with SENTINAL_SESSION_ID,
        // so its rc file could wrap again; refuse and remove that Session.
        let _ = tokio::time::timeout(
            ENTRY_DEADLINE,
            client.request(ClientRequest::Kill {
                session_id: session.id,
            }),
        )
        .await;
        return FALLBACK_EXIT;
    }
    let Some(remaining) = ENTRY_DEADLINE.checked_sub(started.elapsed()) else {
        return FALLBACK_EXIT;
    };
    match client
        .attach_before(session.id, remaining, (cols, rows))
        .await
    {
        Ok(_) => 0,
        Err(_) => FALLBACK_EXIT,
    }
}

fn default_token_file() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config/terminal-agent/token"))
}

fn edit(
    shell: Option<HookShell>,
    rc: Option<PathBuf>,
    enabling: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (rc_env, home) = rc_env()?;
    let (shell, path) = select_rc(shell, rc, &rc_env).map_err(|error| error.to_string())?;
    let now = crate::hook_report::now().0;
    let applied = if enabling {
        let binary = binary()?;
        ensure_not_world_writable(&binary).map_err(|error| error.to_string())?;
        let body = loader_body(shell, &binary).map_err(|error| error.to_string())?;
        apply(&path, now, |original| {
            install_block(original, &body, Markers::AUTO, Placement::Start)
        })
    } else {
        apply(&path, now, |original| {
            uninstall_block(original, Markers::AUTO)
        })
    }
    .map_err(|error| error.to_string())?;
    let (name, file) = (shell.name(), shown(&path, &home));
    let message = match (enabling, applied) {
        (true, Applied::Unchanged) => format!("Auto-wrap is already on for {name} in {file}; nothing changed."),
        (true, Applied::Written { backup }) => format!(
            "Auto-wrap is on for {name}: added a block at the top of {file}{}. New interactive {name} terminals open inside a Sentinal Session; if sentinald does not answer within 200 ms they start normally.",
            backup_note(backup.as_deref(), &home)
        ),
        (false, Applied::Unchanged) => format!("Auto-wrap is not configured in {file}; nothing changed."),
        (false, Applied::Written { backup }) => format!(
            "Auto-wrap is off for {name}: removed the block from {file}{}.",
            backup_note(backup.as_deref(), &home)
        ),
    };
    writeln!(out, "{message}").map_err(|error| error.to_string())
}

fn status(
    shell: Option<HookShell>,
    rc: Option<PathBuf>,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (rc_env, home) = rc_env()?;
    let (shell, path) = select_rc(shell, rc, &rc_env).map_err(|error| error.to_string())?;
    let file = shown(&path, &home);
    let configured = match std::fs::read(&path) {
        Ok(bytes) => match find_block(&bytes, Markers::AUTO) {
            Ok(Some(lines)) => format!(
                "Auto-wrap: on for {} — block at lines {}–{} of {file}",
                shell.name(),
                lines.first,
                lines.last
            ),
            Ok(None) => format!("Auto-wrap: off — no auto block in {file}"),
            Err(error) => format!("Auto-wrap: unknown — {file}: {error}"),
        },
        Err(_) => format!("Auto-wrap: off — {file} does not exist"),
    };
    let current = match std::env::var(SESSION_ENV).ok().filter(|id| !id.is_empty()) {
        Some(id) => format!("This terminal: inside Sentinal Session {id}"),
        None => match skip_reason(&capture_env(Some(shell), true)) {
            Some(reason) => format!(
                "This terminal: a new {} here would not be wrapped — {reason}",
                shell.name()
            ),
            None => format!(
                "This terminal: a new {} here would be wrapped",
                shell.name()
            ),
        },
    };
    writeln!(out, "{configured}\n{current}").map_err(|error| error.to_string())
}

fn backup_note(backup: Option<&Path>, home: &Path) -> String {
    backup.map_or_else(String::new, |backup| {
        format!(" (backup: {})", shown(backup, home))
    })
}
