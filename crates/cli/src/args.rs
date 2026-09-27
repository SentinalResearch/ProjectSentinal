//! Command-line arguments for `ta`.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

/// terminal-agent command-line client.
#[derive(Debug, Parser)]
#[command(name = "ta", version, about)]
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

/// Commands supported by `ta`.
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
}
