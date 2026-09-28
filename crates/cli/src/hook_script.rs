//! External shell hook scripts and the rc-file loader that evaluates them.

use protocol::{render_command_line, ShellDialect};
use std::{
    ffi::OsStr,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

const ZSH_HOOK: &str = include_str!("../../../shell/external/sentinal.zsh");
const BASH_HOOK: &str = include_str!("../../../shell/external/sentinal.bash");
const FISH_HOOK: &str = include_str!("../../../shell/external/sentinal.fish");
const BIN_PLACEHOLDER: &str = "@SENTINAL_BIN@";

/// Shells with an external Sentinal hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum HookShell {
    /// zsh.
    Zsh,
    /// GNU bash.
    Bash,
    /// fish.
    Fish,
}

impl HookShell {
    /// Shell for an executable path such as `$SHELL`, if supported.
    pub fn from_shell_path(shell: &str) -> Option<Self> {
        match ShellDialect::from_shell_path(shell)? {
            ShellDialect::Zsh => Some(HookShell::Zsh),
            ShellDialect::Bash => Some(HookShell::Bash),
            ShellDialect::Fish => Some(HookShell::Fish),
        }
    }

    /// Lower-case shell name used on the command line.
    pub fn name(self) -> &'static str {
        match self {
            HookShell::Zsh => "zsh",
            HookShell::Bash => "bash",
            HookShell::Fish => "fish",
        }
    }

    fn dialect(self) -> ShellDialect {
        match self {
            HookShell::Zsh => ShellDialect::Zsh,
            HookShell::Bash => ShellDialect::Bash,
            HookShell::Fish => ShellDialect::Fish,
        }
    }
}

/// Errors choosing or validating the binary a hook runs.
#[derive(Debug, thiserror::Error)]
pub enum HookScriptError {
    /// A path is writable by every user, so its contents could be replaced.
    #[error("refusing to use '{}': it or its directory is world-writable", .0.display())]
    WorldWritable(PathBuf),
    /// A path could not be inspected.
    #[error("cannot inspect '{}': {source}", path.display())]
    Inspect {
        /// Path being inspected.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// The binary path cannot be written as shell text.
    #[error("cannot quote '{}' for {shell}: {message}", path.display())]
    Quote {
        /// Binary path.
        path: PathBuf,
        /// Target shell.
        shell: &'static str,
        /// Quoting failure.
        message: String,
    },
}

/// The hook script for `shell`, calling the reporter at `binary`.
pub fn render_hook(shell: HookShell, binary: &Path) -> Result<String, HookScriptError> {
    let template = match shell {
        HookShell::Zsh => ZSH_HOOK,
        HookShell::Bash => BASH_HOOK,
        HookShell::Fish => FISH_HOOK,
    };
    Ok(template.replace(BIN_PLACEHOLDER, &quote_path(shell, binary)?))
}

/// The rc-file lines that load the hook at shell start, silently skipping a
/// missing binary so a removed Sentinal never breaks the shell.
pub fn loader_body(shell: HookShell, binary: &Path) -> Result<String, HookScriptError> {
    let quoted = quote_path(shell, binary)?;
    let name = shell.name();
    let load = match shell {
        HookShell::Zsh | HookShell::Bash => {
            format!("if [ -x {quoted} ]; then eval \"$({quoted} hook {name} 2>/dev/null)\"; fi")
        }
        HookShell::Fish => {
            format!("if test -x {quoted}; {quoted} hook {name} 2>/dev/null | source; end")
        }
    };
    Ok(format!(
        "# Managed by `sentinal hook install`; remove with `sentinal hook uninstall`.\n{load}\n"
    ))
}

/// Prefer the `sentinal` found on `PATH` when it is this same executable, so
/// the rc file names a stable path rather than a versioned install location.
pub fn resolve_binary(current_exe: &Path, path_env: Option<&OsStr>) -> PathBuf {
    let Ok(target) = current_exe.canonicalize() else {
        return current_exe.to_path_buf();
    };
    path_env
        .into_iter()
        .flat_map(std::env::split_paths)
        .map(|dir| dir.join("sentinal"))
        .find(|candidate| {
            candidate
                .canonicalize()
                .is_ok_and(|resolved| resolved == target)
        })
        .unwrap_or_else(|| current_exe.to_path_buf())
}

/// Refuse a file (and its directory) that any user could replace.
pub fn ensure_not_world_writable(path: &Path) -> Result<(), HookScriptError> {
    let mut checked = vec![path.to_path_buf()];
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        checked.push(parent.to_path_buf());
    }
    for candidate in checked {
        let metadata =
            std::fs::metadata(&candidate).map_err(|source| HookScriptError::Inspect {
                path: candidate.clone(),
                source,
            })?;
        if metadata.permissions().mode() & 0o002 != 0 {
            return Err(HookScriptError::WorldWritable(candidate));
        }
    }
    Ok(())
}

fn quote_path(shell: HookShell, binary: &Path) -> Result<String, HookScriptError> {
    render_command_line(&[binary.to_string_lossy().into_owned()], shell.dialect()).map_err(
        |error| HookScriptError::Quote {
            path: binary.to_path_buf(),
            shell: shell.name(),
            message: error.to_string(),
        },
    )
}
