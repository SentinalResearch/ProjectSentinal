//! `ta`: compatibility alias for `sentinal`, kept through the Milestone 1b migration.

use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = cli::args::Cli::parse();
    cli::app::run(cli, &mut std::io::stdout(), &mut std::io::stderr()).await
}
