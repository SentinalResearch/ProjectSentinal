//! WebSocket boundary tests for reduced-capture Sessions.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::{AppState, WsAction};
use crate::{
    audit::AuditLog,
    auth::AuthManager,
    pty::FakePty,
    session_manager::{SessionManager, SessionManagerConfig},
};
use protocol::{
    CaptureLevel, ClientRequest, DaemonMessage, Envelope, Machine, MachineId, MachineStatus, Os,
    SessionId, SessionOrigin, SessionState, Timestamp,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

static NEXT_DIR: AtomicU64 = AtomicU64::new(1);

const REDUCED: [(SessionOrigin, CaptureLevel); 3] = [
    (SessionOrigin::ShellHook, CaptureLevel::Commands),
    (SessionOrigin::Adopted, CaptureLevel::ProcessLog),
    (SessionOrigin::Adopted, CaptureLevel::Process),
];

fn state() -> (AppState, Arc<FakePty>, PathBuf) {
    let data_dir = std::env::temp_dir().join(format!(
        "sentinel-api-capture-{}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
    let fake = Arc::new(FakePty::new());
    let manager = SessionManager::new(
        fake.clone(),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 100,
            clock: Arc::new(|| Timestamp(1_790_000_000_000)),
        },
        AuditLog::new(&data_dir),
    );
    let machine = Machine {
        id: MachineId("test-machine".into()),
        name: "Test Machine".into(),
        os: Os::Linux,
        cpu: "test cpu".into(),
        cpu_cores: 8,
        ram_bytes: 1024,
        gpus: vec![],
        status: MachineStatus::Online,
        last_seen: Timestamp(1_790_000_000_000),
    };
    let auth = AuthManager::load(&data_dir.join("auth.json"), "token".into()).unwrap();
    (AppState::new(manager, machine, auth), fake, data_dir)
}

async fn dispatch(state: &AppState, request: ClientRequest) -> WsAction {
    state
        .dispatch_text(&Envelope::new(request).encode().unwrap())
        .await
}

async fn create_ui_session(state: &AppState) -> SessionId {
    let request = ClientRequest::CreateSession {
        shell: Some("/bin/sh".into()),
        cwd: Some("/tmp".into()),
        cols: 80,
        rows: 24,
        origin: SessionOrigin::Ui,
        title: None,
        command: None,
    };
    match dispatch(state, request).await {
        WsAction::Reply(DaemonMessage::SessionUpdated { session }) => session.id,
        _ => panic!("expected SESSION_UPDATED"),
    }
}

fn error_text(action: WsAction) -> String {
    match action {
        WsAction::Reply(DaemonMessage::Error { message }) => message,
        _ => panic!("expected DaemonMessage::Error"),
    }
}

#[tokio::test]
async fn reduced_capture_rejects_attach_resize_kill_and_input_without_side_effects() {
    for (origin, capture) in REDUCED {
        let (state, fake, data_dir) = state();
        let id = create_ui_session(&state).await;
        state
            .sessions
            .set_capture_for_test(id, origin, capture)
            .unwrap();
        let before = state.sessions.session(id).unwrap();

        for request in [
            ClientRequest::Attach { session_id: id },
            ClientRequest::Resize {
                session_id: id,
                cols: 100,
                rows: 30,
            },
            ClientRequest::Kill { session_id: id },
        ] {
            let message = error_text(dispatch(&state, request).await);
            assert!(message.contains(&capture.to_string()), "{message}");
        }
        let input = state
            .sessions
            .write_input(id, b"secret-input-bytes", "100.64.0.2:1234")
            .unwrap_err();
        assert!(input.to_string().contains(&capture.to_string()));

        let after = state.sessions.session(id).unwrap();
        assert_eq!(after, before);
        assert_ne!(after.state, SessionState::Interrupted);
        assert!(!data_dir.join("audit.jsonl").exists());

        // The dispatcher keeps serving the same connection after the rejection.
        assert!(matches!(
            dispatch(&state, ClientRequest::ListSessions).await,
            WsAction::Reply(DaemonMessage::Sessions { sessions }) if sessions.len() == 1
        ));
        fake.exit(id, 0).unwrap();
        if data_dir.exists() {
            std::fs::remove_dir_all(data_dir).unwrap();
        }
    }
}

#[tokio::test]
async fn full_capture_ui_session_keeps_attach_resize_and_input() {
    let (state, fake, data_dir) = state();
    let id = create_ui_session(&state).await;

    assert!(matches!(
        dispatch(&state, ClientRequest::Attach { session_id: id }).await,
        WsAction::Attach { .. }
    ));
    assert!(matches!(
        dispatch(
            &state,
            ClientRequest::Resize {
                session_id: id,
                cols: 100,
                rows: 30
            }
        )
        .await,
        WsAction::Reply(DaemonMessage::SessionUpdated { .. })
    ));
    state
        .sessions
        .write_input(id, b"ls\r", "100.64.0.2:1234")
        .unwrap();
    let audit = std::fs::read_to_string(data_dir.join("audit.jsonl")).unwrap();
    assert!(!audit.contains("ls\\r") && audit.contains("\"byte_count\":3"));
    fake.exit(id, 0).unwrap();
    std::fs::remove_dir_all(data_dir).unwrap();
}
