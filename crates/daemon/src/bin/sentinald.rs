//! `sentinald`: the Sentinal daemon.

#[tokio::main]
async fn main() {
    let result = daemon::entry::run().await;
    if let Err(error) = &result {
        eprintln!("Error: {error:?}");
    }
    // Exit now: dropping the runtime would wait for blocking tasks (PTY
    // readers, log tails), which can outlive SIGTERM and stall launchd/systemd.
    std::process::exit(i32::from(result.is_err()));
}
