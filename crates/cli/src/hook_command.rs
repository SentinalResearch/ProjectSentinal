//! `sentinal hook ...` dispatch: print, install, uninstall, and report.

use crate::{
    args::HookAction,
    hook_report::{self, HookReport, ReportKind, REPORT_DEADLINE, TEXT_ENV},
    hook_script::{ensure_not_world_writable, loader_body, render_hook, resolve_binary, HookShell},
    rc_file,
    rc_install::{apply, select_rc, Applied, RcEnv},
    watch::SESSION_ENV,
};
use protocol::local::SOCKET_ENV;
use std::{
    env,
    io::Write,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// Run one hook operation. `report` never writes anything and always succeeds.
pub async fn run(action: HookAction, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    let result = match action {
        HookAction::Zsh => print(HookShell::Zsh, out),
        HookAction::Bash => print(HookShell::Bash, out),
        HookAction::Fish => print(HookShell::Fish, out),
        HookAction::Install { shell, rc } => install(shell, rc, true, out),
        HookAction::Uninstall { shell, rc } => install(shell, rc, false, out),
        HookAction::Report {
            kind,
            pid,
            shell,
            seq,
            exit,
            at,
        } => {
            report(kind, pid, shell, seq, exit, &at).await;
            return ExitCode::SUCCESS;
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(err, "sentinal: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The `sentinal` binary path to write into rc files.
pub(crate) fn binary() -> Result<PathBuf, String> {
    let current = env::current_exe().map_err(|error| format!("cannot locate sentinal: {error}"))?;
    Ok(resolve_binary(&current, env::var_os("PATH").as_deref()))
}

fn print(shell: HookShell, out: &mut dyn Write) -> Result<(), String> {
    let script = render_hook(shell, &binary()?).map_err(|error| error.to_string())?;
    out.write_all(script.as_bytes())
        .map_err(|error| format!("cannot write the hook: {error}"))
}

fn install(
    shell: Option<HookShell>,
    rc: Option<PathBuf>,
    installing: bool,
    out: &mut dyn Write,
) -> Result<(), String> {
    let (rc_env, home) = rc_env()?;
    let (shell, path) = select_rc(shell, rc, &rc_env).map_err(|error| error.to_string())?;
    let file = shown(&path, &home);
    let applied = if installing {
        let binary = binary()?;
        ensure_not_world_writable(&binary).map_err(|error| error.to_string())?;
        let body = loader_body(shell, &binary).map_err(|error| error.to_string())?;
        apply(&path, hook_report::now().0, |original| {
            rc_file::install(original, &body)
        })
    } else {
        apply(&path, hook_report::now().0, rc_file::uninstall)
    }
    .map_err(|error| error.to_string())?;
    let name = shell.name();
    let message = match (installing, applied) {
        (true, Applied::Unchanged) => {
            format!("The Sentinal {name} hook is already installed in {file}; nothing changed.")
        }
        (true, Applied::Written { backup }) => format!(
            "Installed the Sentinal {name} hook in {file}{}. New {name} terminals report Commands to sentinald.",
            backup_note(backup.as_deref(), &home)
        ),
        (false, Applied::Unchanged) => format!("No Sentinal hook in {file}; nothing changed."),
        (false, Applied::Written { backup }) => format!(
            "Removed the Sentinal hook from {file}{}. Open a new terminal to stop reporting.",
            backup_note(backup.as_deref(), &home)
        ),
    };
    writeln!(out, "{message}").map_err(|error| error.to_string())
}

/// rc-selection facts from this process's environment, plus `$HOME`.
pub(crate) fn rc_env() -> Result<(RcEnv, PathBuf), String> {
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is not set; pass --rc PATH")?;
    let rc_env = RcEnv {
        home: home.clone(),
        shell: env::var("SHELL").ok(),
        zdotdir: env::var_os("ZDOTDIR").map(PathBuf::from),
        xdg_config_home: env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        macos: cfg!(target_os = "macos"),
    };
    Ok((rc_env, home))
}

async fn report(kind: ReportKind, pid: u32, shell: String, seq: u64, exit: i32, at: &str) {
    if hook_report::suppressed(
        env::var(SESSION_ENV).ok().as_deref(),
        env::var("SENTINAL_DISABLE").ok().as_deref(),
    ) {
        return;
    }
    let Some(socket) = hook_report::socket_path(
        env::var_os(SOCKET_ENV).map(PathBuf::from),
        env::var_os("HOME").map(PathBuf::from).as_deref(),
        env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).as_deref(),
    ) else {
        return;
    };
    let cwd = env::var("PWD")
        .ok()
        .filter(|pwd| pwd.starts_with('/'))
        .or_else(|| {
            env::current_dir()
                .ok()
                .map(|dir| dir.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "/".into());
    let text = hook_report::bounded_text(env::var(TEXT_ENV).unwrap_or_default());
    if kind == ReportKind::Start && text.is_empty() {
        return;
    }
    let report = HookReport {
        kind,
        pid,
        shell,
        sequence: seq,
        exit_code: exit,
        at: hook_report::parse_epoch(at).unwrap_or_else(hook_report::now),
        text,
        cwd,
        title: env::var("SENTINAL_TITLE")
            .ok()
            .filter(|title| !title.is_empty()),
    };
    let _ = hook_report::send(&socket, &report, REPORT_DEADLINE).await;
}

/// `path` shown relative to `~` when inside `home`.
pub(crate) fn shown(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn backup_note(backup: Option<&Path>, home: &Path) -> String {
    backup.map_or_else(String::new, |backup| {
        format!(" (backup: {})", shown(backup, home))
    })
}
