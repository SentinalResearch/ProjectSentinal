//! The SQLite database holds Command history and must be owner-only.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use store::{Store, StoreError};

static NEXT_DATABASE: AtomicU64 = AtomicU64::new(1);

fn database_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "terminal-agent-private-store-{}-{}.sqlite3",
        std::process::id(),
        NEXT_DATABASE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn remove_database(path: &Path) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", path.display()));
    }
}

#[test]
fn new_database_and_its_wal_are_owner_only() {
    let path = database_path();

    let store = Store::open(&path).unwrap();

    assert_eq!(mode(&path), 0o600);
    let wal = PathBuf::from(format!("{}-wal", path.display()));
    assert_eq!(mode(&wal), 0o600);
    drop(store);
    remove_database(&path);
}

#[test]
fn existing_world_readable_database_is_tightened_on_open() {
    let path = database_path();
    drop(Store::open(&path).unwrap());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

    let store = Store::open(&path).unwrap();

    assert_eq!(mode(&path), 0o600);
    drop(store);
    remove_database(&path);
}

#[test]
fn database_in_a_missing_directory_returns_a_file_error() {
    let path = database_path().join("missing/terminal-agent.sqlite3");

    let error = Store::open(&path).err().unwrap();

    assert!(matches!(error, StoreError::File { .. }), "{error}");
}
