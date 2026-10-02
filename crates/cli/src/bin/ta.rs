//! `ta`: compatibility alias for `sentinal`, kept through the Milestone 1b migration.

use clap::Parser;

#[tokio::main]
async fn main() -> std::process::ExitCode {
    // Screen argv before clap so a pairing code is never parsed or echoed.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if let Err(message) = cli::connect_command::screen_argv(&args) {
        eprintln!("sentinal: {message}");
        return std::process::ExitCode::from(2);
    }
    let cli = cli::args::Cli::parse_from(args);
    cli::app::run(cli, &mut std::io::stdout(), &mut std::io::stderr()).await
}
