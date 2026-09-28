//! `sentineld`: the Sentinel daemon.

#[tokio::main]
async fn main() -> Result<(), daemon::entry::DaemonError> {
    daemon::entry::run().await
}
