//! One size per Session, set by the latest active viewer (T3.2), through the
//! Session manager: real screen thread, fake PTY, frozen clock.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::{AuditLog, AuditRecord},
    pty::FakePty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig, ViewerOptions},
};
use protocol::{MachineId, SessionOrigin, Timestamp, ViewerMode};
use std::{path::PathBuf, sync::Arc, time::Duration};

struct Harness {
    sessions: SessionManager,
    fake: Arc<FakePty>,
    dir: PathBuf,
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn harness(name: &str) -> Harness {
    let dir = std::env::temp_dir().join(format!("sentinal-size-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let fake = Arc::new(FakePty::new());
    let sessions = SessionManager::new(
        fake.clone(),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: dir.clone(),
            scrollback_lines: 100,
            clock: Arc::new(|| Timestamp(1_790_000_000_000)),
        },
        AuditLog::new(&dir),
    );
    Harness {
        sessions,
        fake,
        dir,
    }
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
        env: None,
        sandbox: false,
    }
}

fn viewer(cols: u16, rows: u16, mode: ViewerMode, address: &str) -> ViewerOptions {
    ViewerOptions {
        size: Some((cols, rows)),
        mode,
        client_addr: address.into(),
    }
}

/// Wait out the settle time.
async fn settled() {
    tokio::time::sleep(Duration::from_millis(400)).await;
}

fn resizes(dir: &std::path::Path) -> Vec<(String, u16, u16)> {
    std::fs::read_to_string(dir.join("audit.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<AuditRecord>(line).ok())
        .filter_map(|record| {
            let resize = record.resize?;
            Some((record.client_addr, resize.cols, resize.rows))
        })
        .collect()
}

#[tokio::test]
async fn two_viewers_typing_in_turn_hand_the_size_over_and_each_is_told() {
    let h = harness("alternate");
    let session = h.sessions.create(request()).await.unwrap();

    let mut laptop = h
        .sessions
        .attach_viewer(
            session.id,
            false,
            viewer(132, 40, ViewerMode::Interactive, "laptop"),
        )
        .unwrap();
    assert_eq!(h.sessions.session_size(session.id).unwrap(), (132, 40));
    let phone = h
        .sessions
        .attach_viewer(
            session.id,
            false,
            viewer(44, 20, ViewerMode::Interactive, "phone"),
        )
        .unwrap();
    assert_eq!(
        h.sessions.session_size(session.id).unwrap(),
        (132, 40),
        "joining does not take the size"
    );

    h.sessions
        .viewer_input(session.id, phone.viewer_id().unwrap())
        .unwrap();
    settled().await;
    assert_eq!(h.sessions.session_size(session.id).unwrap(), (44, 20));

    h.sessions
        .viewer_input(session.id, laptop.viewer_id().unwrap())
        .unwrap();
    settled().await;
    assert_eq!(h.sessions.session_size(session.id).unwrap(), (132, 40));

    assert_eq!(
        resizes(&h.dir),
        [
            ("laptop".to_string(), 132, 40),
            ("phone".to_string(), 44, 20),
            ("laptop".to_string(), 132, 40)
        ],
        "every accepted size is audited with who set it"
    );
    // The PTY itself has the size.
    assert_eq!(h.fake.size(session.id).unwrap(), (132, 40));
    drop(laptop.output.try_recv());
    h.fake.exit(session.id, 0).unwrap();
}

#[tokio::test]
async fn when_the_active_viewer_leaves_the_size_stays() {
    let h = harness("leave");
    let session = h.sessions.create(request()).await.unwrap();
    let laptop = h
        .sessions
        .attach_viewer(
            session.id,
            false,
            viewer(132, 40, ViewerMode::Interactive, "laptop"),
        )
        .unwrap();
    let phone = h
        .sessions
        .attach_viewer(
            session.id,
            false,
            viewer(44, 20, ViewerMode::Interactive, "phone"),
        )
        .unwrap();
    drop(laptop);
    settled().await;
    assert_eq!(h.sessions.session_size(session.id).unwrap(), (132, 40));
    h.sessions
        .viewer_input(session.id, phone.viewer_id().unwrap())
        .unwrap();
    settled().await;
    assert_eq!(h.sessions.session_size(session.id).unwrap(), (44, 20));
    h.fake.exit(session.id, 0).unwrap();
}

#[tokio::test]
async fn an_observer_never_sets_the_size_and_may_not_type() {
    let h = harness("observe");
    let session = h.sessions.create(request()).await.unwrap();
    let peek = h
        .sessions
        .attach_viewer(
            session.id,
            false,
            viewer(60, 18, ViewerMode::Observe, "peek"),
        )
        .unwrap();
    let id = peek.viewer_id().unwrap();
    h.sessions
        .viewer_state(session.id, id, 60, 18, true)
        .unwrap();
    h.sessions.viewer_input(session.id, id).unwrap();
    settled().await;
    assert_eq!(h.sessions.session_size(session.id).unwrap(), (80, 24));
    assert!(!h.sessions.viewer_may_type(session.id, id).unwrap());
    assert!(resizes(&h.dir).is_empty());
    h.fake.exit(session.id, 0).unwrap();
}

#[tokio::test]
async fn an_old_client_resize_still_takes_the_size_at_once() {
    let h = harness("legacy");
    let session = h.sessions.create(request()).await.unwrap();
    let _laptop = h
        .sessions
        .attach_viewer(
            session.id,
            false,
            viewer(132, 40, ViewerMode::Interactive, "laptop"),
        )
        .unwrap();
    // An installed CLI: RESIZE, then ATTACH without a box.
    h.sessions
        .legacy_resize(session.id, None, 100, 30, "old-cli")
        .unwrap();
    assert_eq!(h.sessions.session_size(session.id).unwrap(), (100, 30));
    let old = h.sessions.attach(session.id).unwrap();
    assert!(old.viewer_id().is_some());
    h.fake.exit(session.id, 0).unwrap();
}

#[tokio::test]
async fn a_size_change_redraws_the_screen_at_the_new_size() {
    let h = harness("redraw");
    let session = h.sessions.create(request()).await.unwrap();
    h.fake
        .output(session.id, b"abcdefghijklmnopqrstuvwxyz0123456789")
        .unwrap();
    // The reader draws it on its own thread: wait until it is on the screen.
    tokio::time::timeout(Duration::from_secs(5), async {
        while !h
            .sessions
            .scrollback(session.id)
            .unwrap()
            .iter()
            .any(|line| line.contains("abcdef"))
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("the output reaches the screen");
    let laptop = h
        .sessions
        .attach_viewer(
            session.id,
            false,
            viewer(20, 10, ViewerMode::Interactive, "laptop"),
        )
        .unwrap();
    // Taken alone, before the snapshot: the line wraps at 20 columns.
    let rows = daemon::screen::render_rows(&laptop.screen, 20, 10);
    assert_eq!(rows[0], "abcdefghijklmnopqrst", "{rows:?}");
    assert_eq!(rows[1], "uvwxyz0123456789");
    h.fake.exit(session.id, 0).unwrap();
}
