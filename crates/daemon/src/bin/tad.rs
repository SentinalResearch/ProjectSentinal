//! `tad`: compatibility alias for `sentinald`, kept through the Milestone 1b migration.

#[tokio::main]
async fn main() -> Result<(), daemon::entry::DaemonError> {
    daemon::entry::run().await
}
