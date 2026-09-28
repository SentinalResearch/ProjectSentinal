//! Command-line arguments for `sentinal` (also installed as `ta`).

use crate::{hook_report::ReportKind, hook_script::HookShell};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Sentinal: persistent, observable terminal Sessions.
#[derive(Debug, Parser)]
#[command(name = "sentinal", version, about)]
pub struct Cli {
    /// Daemon WebSocket URL.
    #[arg(long, default_value = "ws://127.0.0.1:7420/api/v1/ws")]
    pub url: String,
    /// Bearer token file; defaults to ~/.config/terminal-agent/token.
    #[arg(long)]
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
    /// Run this terminal inside a persistent Sentinal Session and attach.
    ///
    /// With `-- COMMAND [ARG...]`, the Command runs once in the Session's
    /// shell and the shell stays open afterwards. Detach with Ctrl-\ then d;
    /// closing the window also only detaches.
    Watch {
        /// Session title shown in `ls` and the web UI.
        #[arg(short = 't', long)]
        title: Option<String>,
        /// Command and arguments, passed exactly as given after `--`.
        #[arg(last = true, num_args = 1.., value_name = "COMMAND")]
        command: Option<Vec<String>>,
    },
    /// Report Commands from ordinary zsh, bash, or fish terminals (COMMANDS capture).
    Hook {
        /// Hook operation.
        #[command(subcommand)]
        action: HookAction,
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
