//! Authentication and remote-input audit behaviour tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::{AuditLog, AuditRecord},
    auth::{constant_time_eq, load_or_create_token},
};
use protocol::{SessionId, Timestamp};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "terminal-agent-{name}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn generated_token_is_persisted_and_reused() {
    let dir = temp_dir("token");
    let path = dir.join("token");

    let first = load_or_create_token(&path).unwrap();
    let second = load_or_create_token(&path).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.len(), 64);
    assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn generated_token_is_owner_read_write_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = temp_dir("token-mode");
    let path = dir.join("token");
    load_or_create_token(&path).unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;

    assert_eq!(mode, 0o600);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn token_comparison_accepts_only_an_exact_match() {
    assert!(constant_time_eq("abc123", "abc123"));
    assert!(!constant_time_eq("abc123", "abc124"));
    assert!(!constant_time_eq("abc123", "abc1234"));
}

#[test]
fn audit_log_records_metadata_but_not_terminal_bytes() {
    let dir = temp_dir("audit");
    let log = AuditLog::new(&dir);
    let record = AuditRecord {
        timestamp: Timestamp(1_790_000_000_000),
        client_addr: "100.64.0.2:50123".into(),
        session_id: SessionId(7),
        byte_count: 19,
    };

    log.append(&record).unwrap();

    let line = fs::read_to_string(dir.join("audit.jsonl")).unwrap();
    assert!(line.ends_with('\n'));
    assert!(line.contains("\"session_id\":7"));
    assert!(line.contains("\"byte_count\":19"));
    assert!(!line.contains("password=hunter2"));
    fs::remove_dir_all(dir).unwrap();
}
