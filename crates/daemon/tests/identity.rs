//! Immutable Machine identity: creation, persistence, corruption, and reset.

#![allow(clippy::unwrap_used)]

use daemon::identity::{self, IdentityError};
use protocol::Timestamp;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);

fn path() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sentinal-identity-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    dir.join("machine-id.json")
}

const NOW: Timestamp = Timestamp(1_790_900_000_000);

#[test]
fn first_start_creates_an_owner_only_ulid() {
    let path = path();
    let created = identity::load_or_create(&path, NOW, "old-host").unwrap();

    assert!(created.machine_id.is_ulid(), "{}", created.machine_id);
    assert_eq!(created.legacy_machine_id.as_deref(), Some("old-host"));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn later_starts_return_the_same_id_even_if_the_hostname_changes() {
    let path = path();
    let first = identity::load_or_create(&path, NOW, "old-host").unwrap();
    let second = identity::load_or_create(&path, Timestamp(NOW.0 + 1), "new-host").unwrap();
    assert_eq!(first, second);
}

#[test]
fn new_ids_are_time_ordered_and_distinct() {
    let a = identity::new_ulid(NOW).unwrap();
    let b = identity::new_ulid(NOW).unwrap();
    let later = identity::new_ulid(Timestamp(NOW.0 + 1)).unwrap();
    assert_ne!(a, b);
    assert!(a.0[..10] == b.0[..10], "same millisecond, same time prefix");
    assert!(later.0[..10] > a.0[..10]);
}

#[test]
fn corrupt_files_are_reported_and_never_overwritten() {
    for (name, contents) in [
        ("json", "{not json"),
        (
            "version",
            r#"{"version":2,"machine_id":"01J9Z3K8V6Q2T7M4X5C0RNB8DA","created_at":1,"legacy_machine_id":null}"#,
        ),
        (
            "id",
            r#"{"version":1,"machine_id":"rtx-pc","created_at":1,"legacy_machine_id":null}"#,
        ),
        ("empty", ""),
    ] {
        let path = path();
        fs::write(&path, contents).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        let error = identity::load_or_create(&path, NOW, "host").unwrap_err();

        assert!(
            matches!(error, IdentityError::Corrupt { .. }),
            "{name}: {error}"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            contents,
            "{name} was overwritten"
        );
    }
}

#[test]
fn a_group_readable_identity_file_is_corrupt() {
    let path = path();
    identity::load_or_create(&path, NOW, "host").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        identity::load_or_create(&path, NOW, "host").unwrap_err(),
        IdentityError::Corrupt { .. }
    ));
}

#[test]
fn reset_moves_the_old_file_aside_and_creates_a_new_id() {
    let path = path();
    fs::write(&path, "{broken").unwrap();

    let fresh = identity::reset(&path, NOW, "host").unwrap();

    assert!(fresh.machine_id.is_ulid());
    assert_eq!(identity::load_or_create(&path, NOW, "host").unwrap(), fresh);
    let aside: Vec<_> = fs::read_dir(path.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.starts_with("machine-id.json.corrupt-"))
        .collect();
    assert_eq!(aside.len(), 1);
}

#[test]
fn a_lost_file_yields_a_new_identity() {
    let path = path();
    let first = identity::load_or_create(&path, NOW, "host").unwrap();
    fs::remove_file(&path).unwrap();
    let second = identity::load_or_create(&path, NOW, "host").unwrap();
    assert_ne!(first.machine_id, second.machine_id);
}
