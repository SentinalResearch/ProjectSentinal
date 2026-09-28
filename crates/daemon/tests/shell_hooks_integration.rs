//! Real-shell command tracking integration tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::RealPty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{MachineId, Timestamp};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use store::Store;

async fn exercise(shell: &str) {
    if !Path::new(shell).exists() {
        return;
    }
    let data_dir = std::env::temp_dir().join(format!(
        "terminal-agent-shell-{}-{}",
        Path::new(shell).file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    let store = Arc::new(Store::open_in_memory().unwrap());
    let manager = SessionManager::with_store(
        Arc::new(RealPty),
        SessionManagerConfig {
            machine_id: MachineId("integration-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 100,
            clock: Arc::new(system_timestamp),
        },
        AuditLog::new(&data_dir),
        store.clone(),
    )
    .unwrap();
    let session = manager
        .create(CreateSession {
            shell: Some(shell.into()),
            cwd: Some(PathBuf::from("/")),
            cols: 80,
            rows: 24,
            origin: protocol::SessionOrigin::Ui,
            title: None,
            command: None,
        })
        .await
        .unwrap();

    manager
        .write_input(
            session.id,
            b"true\nfalse\ncd /tmp\nexit\n",
            "local-integration",
        )
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if manager.session(session.id).unwrap().state.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();

    let mut commands = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let commands = store.history(Some(session.id), 10).unwrap();
            let complete = commands
                .iter()
                .filter(|command| {
                    matches!(command.text.as_str(), "true" | "false" | "cd /tmp")
                        && command.exit_code.is_some()
                })
                .count();
            if complete == 3 {
                break commands;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    commands.reverse();
    let tracked: Vec<_> = commands
        .iter()
        .filter(|command| matches!(command.text.as_str(), "true" | "false" | "cd /tmp"))
        .collect();
    assert_eq!(tracked.len(), 3, "tracked commands: {commands:#?}");
    assert_eq!(tracked[0].exit_code, Some(0));
    assert_ne!(tracked[1].exit_code, Some(0));
    assert_eq!(tracked[2].cwd, "/");
    assert_eq!(manager.session(session.id).unwrap().cwd, "/tmp");
    let _ = std::fs::remove_dir_all(data_dir);
}

fn system_timestamp() -> Timestamp {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    Timestamp(i64::try_from(millis).unwrap())
}

#[tokio::test]
#[ignore = "uses a real zsh and PTY"]
async fn zsh_command_tracking_integration() {
    exercise("/bin/zsh").await;
}

#[tokio::test]
#[ignore = "uses a real bash and PTY"]
async fn bash_command_tracking_integration() {
    exercise("/bin/bash").await;
}

#[tokio::test]
#[ignore = "uses a real fish and PTY"]
async fn fish_command_tracking_integration() {
    exercise("/usr/bin/fish").await;
}
