//! Command-line arguments for `sentinal` (also installed as `ta`).

use crate::{hook_report::ReportKind, hook_script::HookShell};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Sentinal: persistent, observable terminal Sessions.
#[derive(Debug, Parser)]
#[command(name = "sentinal", version, about)]
pub struct Cli {
    /// Daemon WebSocket URL.
    #[arg(long, env = "SENTINAL_URL")]
    pub url: Option<String>,
    /// Bearer token file; defaults to ~/.config/terminal-agent/token.
    #[arg(long, env = "SENTINAL_TOKEN_FILE")]
    pub token_file: Option<PathBuf>,
    /// Operation to perform.
    #[command(subcommand)]
    pub command: Command,
}

/// Commands supported by `sentinal`.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// List sessions on the daemon.
    #[command(name = "ls")]
    List,
    /// Create a new persistent session.
    New {
        /// Initial working directory.
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Attach the local terminal to a session.
    Attach {
        /// Session ID.
        id: u64,
    },
    /// Kill a session.
    Kill {
        /// Session ID.
        id: u64,
    },
    /// Check the local worker installation without pairing or changing it.
    Doctor {
        /// Installed program prefix; defaults to ~/.local.
        #[arg(long)]
        prefix: Option<PathBuf>,
        /// Daemon configuration directory; defaults to ~/.config/terminal-agent.
        #[arg(long)]
        config_dir: Option<PathBuf>,
    },
    /// Check for, install, or explicitly roll back a verified worker release.
    Update {
        /// Report whether the stable release differs without installing it.
        #[arg(long, conflicts_with = "rollback")]
        check: bool,
        /// Explicitly permit installing this older stable release.
        #[arg(long, value_name = "VERSION")]
        rollback: Option<String>,
        /// Installed program prefix; defaults to the running binary's prefix.
        #[arg(long)]
        prefix: Option<PathBuf>,
    },
    /// Run this terminal inside a persistent Sentinal Session and attach.
    ///
    /// With `-- COMMAND [ARG...]`, the Command runs once in the Session's
    /// shell and the shell stays open afterwards. Detach with Ctrl+J;
    /// closing the window also only detaches.
    Watch {
        /// Session title shown in `ls` and the web UI.
        #[arg(short = 't', long)]
        title: Option<String>,
        /// Command and arguments, passed exactly as given after `--`.
        #[arg(last = true, num_args = 1.., value_name = "COMMAND")]
        command: Option<Vec<String>>,
    },
    /// Put new interactive terminals into FULL Sessions automatically (opt-in).
    Auto {
        /// Auto-wrap operation.
        #[command(subcommand)]
        action: AutoAction,
    },
    /// Observe an existing same-user process read-only (ADOPTED / PROCESS).
    ///
    /// Sentinal samples the process tree's Metrics and notices when it ends.
    /// It never signals, traces, or writes to the process; the exit code of an
    /// adopted process is always unknown.
    #[command(group(clap::ArgGroup::new("target").required(true).args(["pid", "pick"])))]
    Adopt {
        /// Process ID to adopt.
        pid: Option<u32>,
        /// Choose from your running processes instead of naming a PID.
        #[arg(long)]
        pick: bool,
        /// Log file the process writes (recorded now; tailing arrives later).
        #[arg(long, value_name = "PATH")]
        log: Option<PathBuf>,
        /// Session title shown in `ls` and the web UI.
        #[arg(short = 't', long)]
        title: Option<String>,
    },
    /// Connect this already-installed Machine to an owner-selected control Machine.
    ///
    /// Create a one-time code in the control's Settings → Machines → Add
    /// Machine, then type it at the hidden prompt. The code is never accepted
    /// on the command line.
    Connect {
        /// Control origin, e.g. https://sentinal-control.<tailnet>.ts.net.
        control_url: String,
        /// Display name to propose (defaults to the hostname).
        #[arg(long)]
        name: Option<String>,
        /// Read the pairing code from one stdin line instead of a hidden prompt.
        #[arg(long)]
        code_stdin: bool,
    },
    /// Show or manage this Machine's control link.
    Connection {
        /// Connection operation.
        #[command(subcommand)]
        action: ConnectionAction,
    },
    /// Remove this Machine's control link (the Machine ID is kept).
    Disconnect,
    /// Manage this Machine's immutable identity.
    Identity {
        /// Identity operation.
        #[command(subcommand)]
        action: IdentityAction,
    },
    /// Report Commands from ordinary zsh, bash, or fish terminals (COMMANDS capture).
    Hook {
        /// Hook operation.
        #[command(subcommand)]
        action: HookAction,
    },
}

/// `sentinal connection` operations.
#[derive(Debug, Subcommand)]
pub enum ConnectionAction {
    /// Show whether this Machine is connected to a control.
    Status {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
}

/// `sentinal identity` operations.
#[derive(Debug, Subcommand)]
pub enum IdentityAction {
    /// Replace a corrupt or unwanted Machine ID with a new one.
    Reset {
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

/// `sentinal auto` operations.
#[derive(Debug, Subcommand)]
pub enum AutoAction {
    /// Add the auto-wrap block to your shell's rc file (off by default).
    On {
        /// Shell to configure; defaults to $SHELL.
        shell: Option<HookShell>,
        /// Edit this rc file instead of the default for the shell.
        #[arg(long, value_name = "PATH")]
        rc: Option<PathBuf>,
    },
    /// Remove exactly the auto-wrap block from your shell's rc file.
    Off {
        /// Shell to configure; defaults to $SHELL.
        shell: Option<HookShell>,
        /// Edit this rc file instead of the default for the shell.
        #[arg(long, value_name = "PATH")]
        rc: Option<PathBuf>,
    },
    /// Show where auto-wrap is configured and whether it applies to this terminal.
    Status {
        /// Shell to inspect; defaults to $SHELL.
        shell: Option<HookShell>,
        /// Inspect this rc file instead of the default for the shell.
        #[arg(long, value_name = "PATH")]
        rc: Option<PathBuf>,
    },
    /// Internal: enter an AUTO_WRAP Session from the rc block, or exit 75.
    #[command(hide = true)]
    Enter {
        /// Shell being started.
        #[arg(long)]
        shell: String,
        /// Executable path of that shell.
        #[arg(long)]
        shell_path: String,
    },
}

/// `sentinal hook` operations.
#[derive(Debug, Subcommand)]
pub enum HookAction {
    /// Print the zsh integration without editing any file.
    Zsh,
    /// Print the bash integration without editing any file.
    Bash,
    /// Print the fish integration without editing any file.
    Fish,
    /// Add the Sentinal block to your shell's rc file (idempotent, with backup).
    Install {
        /// Shell to configure; defaults to $SHELL.
        shell: Option<HookShell>,
        /// Edit this rc file instead of the default for the shell.
        #[arg(long, value_name = "PATH")]
        rc: Option<PathBuf>,
    },
    /// Remove exactly the Sentinal block from your shell's rc file.
    Uninstall {
        /// Shell to configure; defaults to $SHELL.
        shell: Option<HookShell>,
        /// Edit this rc file instead of the default for the shell.
        #[arg(long, value_name = "PATH")]
        rc: Option<PathBuf>,
    },
    /// Internal: report one shell event to sentinald (used by the hook).
    #[command(hide = true)]
    Report {
        /// Event kind.
        kind: ReportKind,
        /// Shell PID.
        #[arg(long)]
        pid: u32,
        /// Shell name or path.
        #[arg(long)]
        shell: String,
        /// Command sequence number.
        #[arg(long, default_value_t = 0)]
        seq: u64,
        /// Command exit status.
        #[arg(long, default_value_t = 0, allow_negative_numbers = true)]
        exit: i32,
        /// Event time as Unix seconds with an optional fraction.
        #[arg(long, default_value = "")]
        at: String,
    },
}
