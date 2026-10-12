//! Arguments for `sentinal release` (control releases and updates).

use clap::{Args, Subcommand, ValueEnum};
use std::path::PathBuf;

/// The GitHub repository whose signed releases this control follows.
pub const DEFAULT_RELEASE_REPO: &str = "SentinalResearch/ProjectSentinal";

/// Options shared by every `sentinal release` action.
#[derive(Debug, Clone, Args)]
pub struct ReleaseOptions {
    /// GitHub repository (OWNER/NAME) that publishes signed releases.
    #[arg(long, global = true, default_value = DEFAULT_RELEASE_REPO)]
    pub repo: String,
    /// Pinned release keys file; defaults to the keys compiled into this binary.
    #[arg(long, global = true)]
    pub allowed_signers: Option<PathBuf>,
    /// Installed program prefix; defaults to the running binary's prefix.
    #[arg(long, global = true)]
    pub prefix: Option<PathBuf>,
    /// Daemon config directory; defaults to ~/.config/terminal-agent.
    #[arg(long, global = true)]
    pub config_dir: Option<PathBuf>,
}

/// Release channels a control can follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReleaseChannel {
    /// Published releases only.
    Stable,
    /// Published releases and pre-releases.
    Beta,
}

/// How the control handles new signed releases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ReleasePolicyArg {
    /// Check daily and notify; update only when the owner asks.
    Notify,
    /// Update the control, then idle workers, in the maintenance window.
    Auto,
    /// Never contact GitHub; manual updates only.
    Off,
}

/// `sentinal release` actions.
#[derive(Debug, Subcommand)]
pub enum ReleaseAction {
    /// Look for a newer release signed by a pinned key.
    Check {
        /// Channel to check (defaults to the stored policy's channel).
        #[arg(long, value_enum)]
        channel: Option<ReleaseChannel>,
        /// Print machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Publish and install a newer signed release on this control.
    Update {
        /// Exact version; defaults to the newest signed release on the channel.
        #[arg(long)]
        version: Option<String>,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
        /// Skip GitHub build provenance (the owner signature is still required).
        #[arg(long)]
        skip_provenance: bool,
        /// Seconds to wait for the restarted daemon to answer health checks.
        #[arg(long, default_value_t = 60)]
        health_timeout: u64,
    },
    /// Return this control and its channel to an older published release.
    Rollback {
        /// Exact published version.
        version: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
        /// Seconds to wait for the restarted daemon to answer health checks.
        #[arg(long, default_value_t = 60)]
        health_timeout: u64,
    },
    /// Show or set the update policy and channel.
    Policy {
        /// New policy.
        #[arg(value_enum)]
        policy: Option<ReleasePolicyArg>,
        /// Channel to follow.
        #[arg(long, value_enum)]
        channel: Option<ReleaseChannel>,
    },
}
