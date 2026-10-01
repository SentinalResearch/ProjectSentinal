//! Daemon data (terminal output, audit metadata) is owner-only on disk.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    private_fs::ensure_private_dir,
    pty::FakePty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{MachineId, SessionOrigin, Timestamp};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn temp_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "terminal-agent-private-{name}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

fn manager(data_dir: &Path) -> SessionManager {
    let config = SessionManagerConfig {
        machine_id: MachineId("test-machine".into()),
        data_dir: data_dir.to_path_buf(),
        scrollback_lines: 3,
        clock: Arc::new(|| Timestamp(1_790_000_000_000)),
    };
    SessionManager::new(Arc::new(FakePty::new()), config, AuditLog::new(data_dir))
}

fn request() -> CreateSession {
    CreateSession {
        shell: Some("/bin/sh".into()),
        cwd: Some("/tmp".into()),
        cols: 80,
        rows: 24,
        origin: SessionOrigin::Ui,
        title: None,
        command: None,
    }
}

#[test]
fn missing_data_directory_is_created_owner_only() {
    let root = temp_dir("create");
    let data_dir = root.join("nested/data");

    ensure_private_dir(&data_dir).unwrap();

    assert_eq!(mode(&data_dir), 0o700);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn existing_world_readable_data_directory_is_tightened() {
    let data_dir = temp_dir("tighten");
    fs::create_dir_all(&data_dir).unwrap();
    fs::set_permissions(&data_dir, fs::Permissions::from_mode(0o755)).unwrap();

    ensure_private_dir(&data_dir).unwrap();

    assert_eq!(mode(&data_dir), 0o700);
    fs::remove_dir_all(data_dir).unwrap();
}

#[test]
fn data_path_that_is_a_file_is_rejected() {
    let path = temp_dir("file");
    fs::write(&path, b"not a directory").unwrap();

    let error = ensure_private_dir(&path).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists, "{error}");
    fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn session_output_log_and_its_directory_are_owner_only() {
    let data_dir = temp_dir("session-log");
    let manager = manager(&data_dir);

    let session = manager.create(request()).await.unwrap();

    let directory_mode = mode(&data_dir.join("sessions"));
    let log_mode = mode(&data_dir.join(format!("sessions/{}.log.zst", session.id.0)));
    manager.kill(session.id).unwrap();
    fs::remove_dir_all(data_dir).unwrap();
    assert_eq!(directory_mode, 0o700);
    assert_eq!(log_mode, 0o600);
}

#[tokio::test]
async fn remote_input_audit_log_is_owner_only() {
    let data_dir = temp_dir("audit");
    let manager = manager(&data_dir);
    let session = manager.create(request()).await.unwrap();

    manager
        .write_input(session.id, b"ls\n", "100.64.0.2:5000")
        .unwrap();

    let audit_mode = mode(&data_dir.join("audit.jsonl"));
    manager.kill(session.id).unwrap();
    fs::remove_dir_all(data_dir).unwrap();
    assert_eq!(audit_mode, 0o600);
}
