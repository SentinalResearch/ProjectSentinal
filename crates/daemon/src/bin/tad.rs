//! `tad`: compatibility alias for `sentinald`, kept through the Milestone 1b migration.

#[tokio::main]
async fn main() {
    let result = daemon::entry::run().await;
    if let Err(error) = &result {
        eprintln!("Error: {error:?}");
    }
    // Same prompt exit as `sentinald`: never wait on blocking PTY readers.
    std::process::exit(i32::from(result.is_err()));
}
