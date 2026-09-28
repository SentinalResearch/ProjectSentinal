//! Auto-wrap: whether a new interactive shell enters an AUTO_WRAP Session.
//!
//! The rc block runs `sentinal auto enter` as a child of the starting shell.
//! Exit status 0 means the terminal was wrapped and the wrapped shell has
//! ended or been detached, so the rc block exits the outer shell. Any other
//! status (normally [`FALLBACK_EXIT`]) means "continue": the original shell
//! keeps starting exactly as if Sentinal were absent. Nothing is ever exec'd
//! or relaunched, so argv, environment, cwd, and exit behavior are untouched.

use crate::hook_script::{HookScriptError, HookShell};
use protocol::{render_command_line, ShellDialect};
use std::{future::Future, path::Path, time::Duration};

/// Exit status meaning "not wrapped; continue the original shell" (EX_TEMPFAIL).
pub const FALLBACK_EXIT: u8 = 75;
/// Time allowed to reach the daemon and create the Session. Leaves headroom
/// under the 300 ms fail-open budget for process start and the skip checks.
pub const ENTRY_DEADLINE: Duration = Duration::from_millis(250);

/// Why a shell is not auto-wrapped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    /// `SENTINAL_DISABLE=1`.
    Disabled,
    /// `SENTINAL_SESSION_ID` is set: already inside a Sentinal Session.
    InsideSession,
    /// `SENTINAL_AUTO_GUARD` is set (set in every AUTO_WRAP shell).
    RecursionGuard,
    /// The shell is not interactive.
    NotInteractive,
    /// stdin or stdout is not a terminal.
    NoTerminal,
    /// An scp/rsync/sftp transfer over SSH.
    FileTransfer,
    /// SSH without a terminal, or running a forced/remote command.
    NonInteractiveSsh,
    /// An IDE is resolving the login environment (named variable).
    IdeEnvironment(&'static str),
    /// A variable listed in `SENTINAL_AUTO_SKIP` is set (named variable).
    Configured(String),
    /// `TERM` is unset or `dumb`.
    DumbTerminal,
    /// Not zsh, bash, or fish.
    UnsupportedShell,
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::Disabled => write!(formatter, "SENTINAL_DISABLE=1 is set"),
            SkipReason::InsideSession => write!(formatter, "already inside a Sentinal Session"),
            SkipReason::RecursionGuard => write!(formatter, "SENTINAL_AUTO_GUARD is set"),
            SkipReason::NotInteractive => write!(formatter, "the shell is not interactive"),
            SkipReason::NoTerminal => write!(formatter, "stdin or stdout is not a terminal"),
            SkipReason::FileTransfer => write!(formatter, "this is an scp/rsync/sftp transfer"),
            SkipReason::NonInteractiveSsh => {
                write!(formatter, "this is a non-interactive SSH command")
            }
            SkipReason::IdeEnvironment(var) => {
                write!(formatter, "an IDE is reading the environment ({var})")
            }
            SkipReason::Configured(var) => {
                write!(formatter, "{var} is set and listed in SENTINAL_AUTO_SKIP")
            }
            SkipReason::DumbTerminal => write!(formatter, "TERM is unset or dumb"),
            SkipReason::UnsupportedShell => {
                write!(formatter, "the shell is not zsh, bash, or fish")
            }
        }
    }
}

/// Everything the decision depends on, captured once.
#[derive(Debug, Clone, Default)]
pub struct AutoEnv {
    /// Relevant environment variables (unset variables are absent).
    pub vars: std::collections::BTreeMap<String, String>,
    /// The shell reports itself interactive.
    pub interactive: bool,
    /// stdin is a terminal.
    pub stdin_tty: bool,
    /// stdout is a terminal.
    pub stdout_tty: bool,
    /// The shell being started, if supported.
    pub shell: Option<HookShell>,
}

impl AutoEnv {
    fn var(&self, name: &str) -> Option<&str> {
        self.vars
            .get(name)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }
}

/// Environment variables the decision reads (for capturing a snapshot).
pub const SIGNAL_VARS: &[&str] = &[
    "SENTINAL_DISABLE",
    "SENTINAL_SESSION_ID",
    "SENTINAL_AUTO_GUARD",
    "SENTINAL_AUTO_SKIP",
    "SSH_ORIGINAL_COMMAND",
    "SSH_CONNECTION",
    "SSH_CLIENT",
    "SSH_TTY",
    "VSCODE_RESOLVING_ENVIRONMENT",
    "INTELLIJ_ENVIRONMENT_READER",
    "TERM",
];

/// IDE variables set only while an IDE runs a shell to read its environment.
/// Ordinary IDE terminals (`TERM_PROGRAM=vscode`, JetBrains JediTerm) are
/// deliberately not skipped.
const IDE_ENVIRONMENT_READERS: &[&str] = &[
    "VSCODE_RESOLVING_ENVIRONMENT",
    "INTELLIJ_ENVIRONMENT_READER",
];

type Rule = fn(&AutoEnv) -> Option<SkipReason>;

/// Skip rules in precedence order; the first match wins.
const RULES: &[Rule] = &[
    |env| (env.var("SENTINAL_DISABLE") == Some("1")).then_some(SkipReason::Disabled),
    |env| {
        env.var("SENTINAL_SESSION_ID")
            .map(|_| SkipReason::InsideSession)
    },
    |env| {
        env.var("SENTINAL_AUTO_GUARD")
            .map(|_| SkipReason::RecursionGuard)
    },
    |env| (!env.interactive).then_some(SkipReason::NotInteractive),
    |env| (!(env.stdin_tty && env.stdout_tty)).then_some(SkipReason::NoTerminal),
    |env| {
        let command = env.var("SSH_ORIGINAL_COMMAND")?;
        let program = command.split_whitespace().next()?.rsplit('/').next()?;
        matches!(program, "scp" | "rsync" | "sftp" | "sftp-server")
            .then_some(SkipReason::FileTransfer)
    },
    |env| {
        let ssh = env
            .var("SSH_CONNECTION")
            .or(env.var("SSH_CLIENT"))
            .is_some();
        (env.var("SSH_ORIGINAL_COMMAND").is_some() || (ssh && env.var("SSH_TTY").is_none()))
            .then_some(SkipReason::NonInteractiveSsh)
    },
    |env| {
        IDE_ENVIRONMENT_READERS
            .iter()
            .find(|name| env.var(name).is_some())
            .map(|name| SkipReason::IdeEnvironment(name))
    },
    |env| {
        env.var("SENTINAL_AUTO_SKIP")?
            .split(',')
            .map(str::trim)
            .find(|name| !name.is_empty() && env.var(name).is_some())
            .map(|name| SkipReason::Configured(name.to_string()))
    },
    |env| matches!(env.var("TERM"), None | Some("dumb")).then_some(SkipReason::DumbTerminal),
    |env| env.shell.is_none().then_some(SkipReason::UnsupportedShell),
];

/// `None` means wrap; otherwise the first applicable skip reason.
pub fn skip_reason(env: &AutoEnv) -> Option<SkipReason> {
    RULES.iter().find_map(|rule| rule(env))
}

/// Run `work` but give up at `deadline`; a result that arrives later is
/// dropped unobserved, so it can never start a second shell here.
pub async fn within<T>(deadline: Duration, work: impl Future<Output = Option<T>>) -> Option<T> {
    tokio::time::timeout(deadline, work).await.ok().flatten()
}

/// The rc-file lines that try to enter an AUTO_WRAP Session at shell start.
pub fn loader_body(shell: HookShell, binary: &Path) -> Result<String, HookScriptError> {
    let dialect = match shell {
        HookShell::Zsh => ShellDialect::Zsh,
        HookShell::Bash => ShellDialect::Bash,
        HookShell::Fish => ShellDialect::Fish,
    };
    let quoted = render_command_line(&[binary.to_string_lossy().into_owned()], dialect).map_err(
        |error| HookScriptError::Quote {
            path: binary.to_path_buf(),
            shell: shell.name(),
            message: error.to_string(),
        },
    )?;
    let header = "# Managed by `sentinal auto on`; remove with `sentinal auto off`.\n";
    let load = match shell {
        HookShell::Zsh | HookShell::Bash => {
            let path = if shell == HookShell::Zsh {
                "${commands[zsh]:-zsh}"
            } else {
                "${BASH:-bash}"
            };
            let name = shell.name();
            format!(
                "if [ -z \"${{SENTINAL_SESSION_ID-}}\" ] && [ \"${{SENTINAL_DISABLE-}}\" != 1 ] && [ -z \"${{SENTINAL_AUTO_GUARD-}}\" ] && [ -x {quoted} ]; then\n  case $- in *i*) {quoted} auto enter --shell {name} --shell-path \"{path}\" && exit 0 ;; esac\nfi\n"
            )
        }
        HookShell::Fish => format!(
            "if status is-interactive; and not set -q SENTINAL_SESSION_ID; and test \"$SENTINAL_DISABLE\" != 1; and not set -q SENTINAL_AUTO_GUARD; and test -x {quoted}\n    {quoted} auto enter --shell fish --shell-path (status fish-path); and exit 0\nend\n"
        ),
    };
    Ok(format!("{header}{load}"))
}
