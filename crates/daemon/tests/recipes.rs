//! Recipe loading and daemon-side execution safety.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    api::{AppState, WsAction},
    audit::AuditLog,
    auth::AuthManager,
    pty::FakePty,
    recipes::RecipeCatalog,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{
    ClientRequest, DaemonMessage, Envelope, Machine, MachineId, MachineStatus, Os, SessionId,
    Timestamp,
};
use std::{collections::BTreeMap, fs, sync::Arc, time::Duration};

#[tokio::test]
async fn run_recipe_quotes_injection_value_before_exact_pty_write() {
    let root = std::env::temp_dir().join(format!("sentinal-recipe-api-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("safe.toml"),
        "id='safe'\nname='Safe'\ncommand='echo {value}'\n[[params]]\nname='value'\n",
    )
    .unwrap();
    let fake = Arc::new(FakePty::new());
    let sessions = SessionManager::new(
        fake.clone(),
        SessionManagerConfig {
            machine_id: MachineId("machine-a".into()),
            data_dir: root.join("data"),
            scrollback_lines: 100,
            clock: Arc::new(|| Timestamp(1_790_000_000_000)),
        },
        AuditLog::new(&root.join("data")),
    );
    let (catalog, diagnostics) = RecipeCatalog::load(Some(root.clone()));
    assert!(diagnostics.is_empty());
    let state = AppState::new(
        sessions,
        Machine {
            id: MachineId("machine-a".into()),
            name: "Machine A".into(),
            os: Os::Linux,
            cpu: "test".into(),
            cpu_cores: 1,
            ram_bytes: 1,
            gpus: vec![],
            status: MachineStatus::Online,
            last_seen: Timestamp(1_790_000_000_000),
            role: None,
        },
        AuthManager::load(&root.join("auth.json"), "token".into()).unwrap(),
    )
    .with_recipes(catalog);
    let mut values = BTreeMap::new();
    values.insert("value".into(), "x; touch /tmp/pwn".into());
    let action = state
        .dispatch_text(
            &Envelope::new(ClientRequest::RunRecipe {
                recipe_id: "safe".into(),
                values,
                session_id: None,
            })
            .encode()
            .unwrap(),
        )
        .await;
    let session = match action {
        WsAction::Reply(DaemonMessage::SessionUpdated { session }) => session,
        _ => panic!("RUN_RECIPE did not create a Session"),
    };
    let spawn = fake.spawn_request(session.id).unwrap();
    let nonce = spawn
        .env
        .iter()
        .find(|(key, _)| key == "SENTINAL_OSC_NONCE")
        .unwrap()
        .1
        .clone();
    fake.output(
        session.id,
        format!("\x1b]133;A;sentinal={nonce}\x07% ").as_bytes(),
    )
    .unwrap();
    for _ in 0..20 {
        if !fake.input(session.id).unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(
        fake.input(SessionId(session.id.0)).unwrap(),
        b"echo 'x; touch /tmp/pwn'\r"
    );
    fake.exit(session.id, 0).unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let _ = fs::remove_dir_all(root);
}
