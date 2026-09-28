//! Command-line arguments for `sentinel` (also installed as `ta`).

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// Sentinel: persistent, observable terminal Sessions.
#[derive(Debug, Parser)]
#[command(name = "sentinel", version, about)]
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

/// Commands supported by `sentinel`.
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
    /// Run this terminal inside a persistent Sentinel Session and attach.
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
}
