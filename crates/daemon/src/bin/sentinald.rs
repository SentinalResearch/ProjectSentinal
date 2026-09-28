//! `sentinald`: the Sentinal daemon.

#[tokio::main]
async fn main() -> Result<(), daemon::entry::DaemonError> {
    daemon::entry::run().await
}
