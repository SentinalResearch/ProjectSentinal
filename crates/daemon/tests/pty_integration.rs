//! Real-shell PTY integration tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::RealPty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{MachineId, SessionState};
use std::{fs, sync::Arc, time::Duration};

/// A real child continues while detached and its final screen survives reattachment.
#[tokio::test]
#[ignore = "uses a real PTY and shell"]
async fn persistent_session_integration() {
    let data_dir =
        std::env::temp_dir().join(format!("terminal-agent-real-pty-{}", std::process::id()));
    let manager = SessionManager::new(
        Arc::new(RealPty),
        SessionManagerConfig::system(
            MachineId("integration-machine".into()),
            data_dir.clone(),
            100,
        ),
        AuditLog::new(&data_dir),
    );
    let session = manager
        .create(CreateSession {
            shell: Some("/bin/sh".into()),
            cwd: Some("/tmp".into()),
            cols: 80,
            rows: 24,
            origin: protocol::SessionOrigin::Ui,
            title: None,
            command: None,
        })
        .await
        .unwrap();
    let mut attached = manager.attach(session.id).unwrap();
    manager
        .write_input(
            session.id,
            b"sh -c 'echo ready; sleep 2; echo done'\nexit 0\n",
            "integration-test",
        )
        .unwrap();

    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let bytes = attached.output.recv().await.unwrap();
            if String::from_utf8_lossy(&bytes).contains("ready") {
                return;
            }
        }
    })
    .await
    .unwrap();
    drop(attached);

    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if manager.session(session.id).unwrap().state == SessionState::Completed {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let reattached = manager.attach(session.id).unwrap();
    let mut restored = vt100::Parser::new(24, 80, 0);
    restored.process(&reattached.screen);

    assert!(restored.screen().contents().contains("done"));
    fs::remove_dir_all(data_dir).unwrap();
}
