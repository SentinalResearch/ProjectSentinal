//! AUTO_WRAP and WATCH Sessions start with their terminal's environment.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::{FakePty, RealPty},
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{MachineId, SessionOrigin, Timestamp};
use std::{path::PathBuf, sync::Arc, time::Duration};

fn data_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "sentinal-session-env-{name}-{}",
        std::process::id()
    ))
}

fn fake_manager(name: &str) -> (SessionManager, Arc<FakePty>) {
    let dir = data_dir(name);
    let backend = Arc::new(FakePty::new());
    let config = SessionManagerConfig {
        machine_id: MachineId("test-machine".into()),
        data_dir: dir.clone(),
        scrollback_lines: 3,
        clock: Arc::new(|| Timestamp(1_790_000_000_000)),
    };
    (
        SessionManager::new(backend.clone(), config, AuditLog::new(&dir)),
        backend,
    )
}

fn request(
    origin: SessionOrigin,
    shell: &str,
    env: Option<Vec<(String, String)>>,
) -> CreateSession {
    CreateSession {
        shell: Some(shell.into()),
        cwd: Some("/tmp".into()),
        cols: 80,
        rows: 24,
        origin,
        title: None,
        command: None,
        env,
    }
}

fn pair(name: &str, value: &str) -> (String, String) {
    (name.into(), value.into())
}

#[tokio::test]
async fn auto_wrap_shells_start_from_the_terminal_environment() {
    let (manager, fake) = fake_manager("auto");
    let terminal = vec![
        pair("PATH", "/opt/homebrew/bin:/usr/bin:/bin"),
        pair("ZDOTDIR", "/home/me/.config/zsh"),
        pair("SENTINAL_SESSION_ID", "999"),
    ];
    let session = manager
        .create(request(SessionOrigin::AutoWrap, "/bin/zsh", Some(terminal)))
        .await
        .unwrap();
    let spawned = fake.spawn_request(session.id).unwrap();
    let base = spawned.base_env.expect("the terminal's environment");
    assert!(base.contains(&pair("PATH", "/opt/homebrew/bin:/usr/bin:/bin")));
    assert!(
        !base.iter().any(|(name, _)| name == "SENTINAL_SESSION_ID"),
        "daemon-owned variables come only from the daemon"
    );
    assert!(spawned
        .env
        .contains(&pair("SENTINAL_SESSION_ID", &session.id.0.to_string())));
    assert!(
        spawned
            .env
            .contains(&pair("TA_USER_ZDOTDIR", "/home/me/.config/zsh")),
        "the terminal's ZDOTDIR is the one whose .zshrc is loaded"
    );
    fake.exit(session.id, 0).unwrap();
}

#[tokio::test]
async fn dashboard_sessions_start_from_the_login_environment() {
    let (manager, fake) = fake_manager("ui");
    let session = manager
        .create(request(SessionOrigin::Ui, "/bin/sh", None))
        .await
        .unwrap();
    fake.exit(session.id, 0).unwrap();
    assert_eq!(
        fake.spawn_request(session.id).unwrap().base_env,
        None,
        "no login environment: the daemon's is kept"
    );

    fake.set_login_env(vec![
        pair("PATH", "/opt/homebrew/bin:/usr/bin"),
        pair("SENTINAL_OSC_NONCE", "forged"),
    ]);
    let session = manager
        .create(request(SessionOrigin::Ui, "/bin/sh", None))
        .await
        .unwrap();
    assert_eq!(
        fake.spawn_request(session.id).unwrap().base_env,
        Some(vec![pair("PATH", "/opt/homebrew/bin:/usr/bin")])
    );
    fake.exit(session.id, 0).unwrap();
}

#[tokio::test]
#[ignore = "uses a real PTY and shell"]
async fn a_real_shell_sees_only_the_terminal_environment() {
    let dir = data_dir("real");
    let manager = SessionManager::new(
        Arc::new(RealPty),
        SessionManagerConfig::system(MachineId("integration-machine".into()), dir.clone(), 100),
        AuditLog::new(&dir),
    );
    let terminal = vec![
        pair("PATH", "/usr/bin:/bin"),
        pair("SENTINAL_FROM_TERMINAL", "yes"),
    ];
    let session = manager
        .create(request(SessionOrigin::AutoWrap, "/bin/sh", Some(terminal)))
        .await
        .unwrap();
    let mut attached = manager.attach(session.id).unwrap();
    // cargo sets CARGO_PKG_NAME for this test process, so the daemon has it;
    // the terminal environment does not, so the shell must not either.
    manager
        .write_input(
            session.id,
            b"echo \"<$SENTINAL_FROM_TERMINAL|${CARGO_PKG_NAME-unset}>\"; exit 0\n",
            "integration-test",
        )
        .unwrap();
    let mut seen = String::new();
    tokio::time::timeout(Duration::from_secs(5), async {
        while !seen.contains("<yes|unset>") {
            let bytes = attached.output.recv().await.unwrap();
            seen.push_str(&String::from_utf8_lossy(&bytes));
        }
    })
    .await
    .unwrap_or_else(|_| panic!("shell output: {seen:?}"));
    drop(attached);
    let _ = std::fs::remove_dir_all(dir);
}
