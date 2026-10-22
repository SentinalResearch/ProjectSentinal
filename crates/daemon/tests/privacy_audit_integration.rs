//! Release gate: raw keystroke payloads are never stored on disk.
//!
//! A real shell reads a password with echo off. Afterwards every file in the
//! daemon data directory — SQLite and its WAL, audit logs, and the
//! decompressed Session output log — is scanned for the typed secret.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::RealPty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{MachineId, SessionOrigin, Timestamp};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use store::Store;

const CANARY: &str = "hunter2-PRIVACY-CANARY-7f3a";

fn now() -> Timestamp {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    Timestamp(i64::try_from(millis).unwrap())
}

fn files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files(&path));
        } else {
            found.push(path);
        }
    }
    found
}

fn readable_bytes(path: &Path) -> Vec<u8> {
    let raw = fs::read(path).unwrap();
    if path.extension().is_some_and(|extension| extension == "zst") {
        zstd::stream::decode_all(raw.as_slice()).unwrap_or(raw)
    } else {
        raw
    }
}

async fn wait_for_output(manager: &SessionManager, id: protocol::SessionId, text: &str) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !manager
            .scrollback(id)
            .unwrap()
            .iter()
            .any(|line| line.contains(text))
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("shell never printed {text:?}"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "runs a real shell and PTY"]
async fn privacy_audit_integration() {
    let data_dir = std::env::temp_dir().join(format!("sentinal-privacy-{}", std::process::id()));
    let store = Arc::new(
        Store::open(&{
            fs::create_dir_all(&data_dir).unwrap();
            data_dir.join("terminal-agent.sqlite3")
        })
        .unwrap(),
    );
    let manager = SessionManager::with_store(
        Arc::new(RealPty),
        SessionManagerConfig {
            machine_id: MachineId("privacy".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 200,
            clock: Arc::new(now),
        },
        AuditLog::new(&data_dir),
        store.clone(),
    )
    .unwrap();
    let session = manager
        .create(CreateSession {
            shell: Some("/bin/bash".into()),
            cwd: Some("/".into()),
            cols: 100,
            rows: 30,
            origin: SessionOrigin::Ui,
            title: None,
            command: None,
            env: None,
        })
        .await
        .unwrap();

    // Type the secret only once echo is provably off: the marker is printed
    // after `stty -echo` ran (slow rc files would otherwise race the input).
    manager
        .write_input(
            session.id,
            b"stty -echo; echo ARMED-$((40+2)); read -r pw; stty echo; echo \"len=${#pw}\"\r",
            "100.64.0.2:1",
        )
        .unwrap();
    wait_for_output(&manager, session.id, "ARMED-42").await;
    manager
        .write_input(session.id, format!("{CANARY}\r").as_bytes(), "100.64.0.2:1")
        .unwrap();
    wait_for_output(&manager, session.id, &format!("len={}", CANARY.len())).await;
    manager
        .write_input(session.id, b"exit\r", "100.64.0.2:1")
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        while !manager.session(session.id).unwrap().state.is_finished() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    drop(manager);
    drop(store);

    let scanned = files(&data_dir);
    let audit = fs::read_to_string(data_dir.join("audit.jsonl")).unwrap();
    for path in &scanned {
        let bytes = readable_bytes(path);
        assert!(
            !bytes
                .windows(CANARY.len())
                .any(|window| window == CANARY.as_bytes()),
            "raw keystroke payload found in {}",
            path.display()
        );
        let shell_hook = path.components().any(|part| part.as_os_str() == "shell");
        if !shell_hook {
            let mode = fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode & 0o077, 0, "{} is mode {mode:o}", path.display());
        }
    }
    assert!(scanned
        .iter()
        .any(|path| path.ends_with("terminal-agent.sqlite3")));
    assert!(scanned
        .iter()
        .any(|path| path.to_string_lossy().ends_with(".log.zst")));
    // Each input frame is audited by size alone (the canary line is 28 bytes).
    assert!(
        audit.contains(&format!("\"byte_count\":{}", CANARY.len() + 1)),
        "{audit}"
    );
    let _ = fs::remove_dir_all(data_dir);
}
