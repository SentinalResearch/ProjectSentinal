//! Watch Session creation and initial Command behavior with the fake PTY.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::FakePty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{
    render_command_line, CaptureLevel, MachineId, SessionId, SessionOrigin, SessionState,
    ShellDialect, Timestamp,
};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use store::Store;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);
const PROMPT: &[u8] = b"\x1b]133;A\x07% ";

struct Harness {
    manager: SessionManager,
    fake: Arc<FakePty>,
    store: Arc<Store>,
    data_dir: PathBuf,
}

fn harness() -> Harness {
    let data_dir = std::env::temp_dir().join(format!(
        "sentinel-watch-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&data_dir).unwrap();
    let fake = Arc::new(FakePty::new());
    let store = Arc::new(Store::open(&data_dir.join("sentinel.sqlite3")).unwrap());
    let manager = SessionManager::with_store(
        fake.clone(),
        SessionManagerConfig {
            machine_id: MachineId("test-machine".into()),
            data_dir: data_dir.clone(),
            scrollback_lines: 100,
            clock: Arc::new(|| Timestamp(1_790_000_000_000)),
        },
        AuditLog::new(&data_dir),
        store.clone(),
    )
    .unwrap();
    Harness {
        manager,
        fake,
        store,
        data_dir,
    }
}

fn watch(command: Option<Vec<String>>) -> CreateSession {
    CreateSession {
        shell: Some("/bin/zsh".into()),
        cwd: Some("/work/project".into()),
        cols: 132,
        rows: 43,
        origin: SessionOrigin::Watch,
        title: Some("Qwen Training".into()),
        command,
    }
}

fn argv(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for (index, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if index <= chunk.len() {
                out.push(char::from(TABLE[((n >> shift) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Emit a marker and wait for it, proving every earlier chunk was processed.
async fn sync(h: &Harness, id: SessionId) {
    let mut attachment = h.manager.attach(id).unwrap();
    h.fake.output(id, b"<sync>").unwrap();
    tokio::time::timeout(Duration::from_millis(100), async {
        loop {
            if attachment.output.recv().await.unwrap() == b"<sync>" {
                return;
            }
        }
    })
    .await
    .unwrap();
}

async fn wait_until(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_millis(100), async {
        while !condition() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}

fn cleanup(harness: Harness, id: SessionId) {
    let _ = harness.fake.exit(id, 0);
    fs::remove_dir_all(harness.data_dir).unwrap();
}

#[tokio::test]
async fn watch_session_records_origin_capture_title_cwd_pid_and_session_env() {
    let h = harness();
    let session = h.manager.create(watch(None)).await.unwrap();

    assert_eq!(session.origin, SessionOrigin::Watch);
    assert_eq!(session.capture, CaptureLevel::Full);
    assert_eq!(session.title.as_deref(), Some("Qwen Training"));
    assert_eq!(session.cwd, "/work/project");
    assert_eq!(session.state, SessionState::Idle);
    assert!(session.pid.is_some());
    assert_eq!(
        h.manager.process_roots().unwrap(),
        vec![(session.id, session.pid.unwrap())]
    );

    let spawn = h.fake.spawn_request(session.id).unwrap();
    assert_eq!((spawn.cols, spawn.rows), (132, 43));
    assert!(spawn
        .env
        .contains(&("SENTINEL_SESSION_ID".into(), session.id.0.to_string())));
    // A bare Watch never types anything into the shell.
    h.fake.output(session.id, PROMPT).unwrap();
    sync(&h, session.id).await;
    assert!(h.fake.input(session.id).unwrap().is_empty());
    cleanup(h, session.id);
}

#[tokio::test]
async fn ordinary_ui_sessions_also_identify_themselves_to_prevent_nesting() {
    let h = harness();
    let mut request = watch(None);
    request.origin = SessionOrigin::Ui;
    request.title = None;
    let session = h.manager.create(request).await.unwrap();

    let spawn = h.fake.spawn_request(session.id).unwrap();
    assert!(spawn
        .env
        .contains(&("SENTINEL_SESSION_ID".into(), session.id.0.to_string())));
    assert_eq!(session.origin, SessionOrigin::Ui);
    cleanup(h, session.id);
}

#[tokio::test]
async fn hostile_initial_command_is_typed_once_as_quoted_data_after_prompt() {
    let h = harness();
    let command = argv(&[
        "python",
        "train.py",
        "$(touch /tmp/pwned)",
        "a; rm -rf ~",
        "line\nbreak",
        "it's",
        "",
        "日本語",
        "\u{3}\r",
    ]);
    let expected = format!(
        "{}\r",
        render_command_line(&command, ShellDialect::Zsh).unwrap()
    );
    let session = h.manager.create(watch(Some(command))).await.unwrap();

    // Nothing is typed before the shell reports readiness.
    h.fake.output(session.id, b"loading rc files\r\n").unwrap();
    sync(&h, session.id).await;
    assert!(h.fake.input(session.id).unwrap().is_empty());
    h.fake.output(session.id, PROMPT).unwrap();
    wait_until(|| !h.fake.input(session.id).unwrap().is_empty()).await;
    let typed = h.fake.input(session.id).unwrap();
    assert_eq!(typed, expected.as_bytes());
    assert_eq!(typed.iter().filter(|byte| **byte == b'\r').count(), 1);
    assert!(typed[..typed.len() - 1]
        .iter()
        .all(|byte| (0x20..0x7f).contains(byte)));

    // A later prompt never repeats the initial Command.
    h.fake.output(session.id, PROMPT).unwrap();
    sync(&h, session.id).await;
    assert_eq!(h.fake.input(session.id).unwrap(), expected.as_bytes());
    cleanup(h, session.id);
}

#[tokio::test]
async fn initial_command_is_tracked_once_persisted_and_shell_returns_idle() {
    let h = harness();
    let command = argv(&["python", "long_job.py", "--epochs", "3"]);
    let text = render_command_line(&command, ShellDialect::Zsh).unwrap();
    let session = h.manager.create(watch(Some(command))).await.unwrap();

    h.fake.output(session.id, PROMPT).unwrap();
    wait_until(|| !h.fake.input(session.id).unwrap().is_empty()).await;
    // The shell echoes and runs the typed line, then reports it through OSC.
    let run = format!(
        "\x1b]7420;cmd={}\x07\x1b]133;B\x07\x1b]133;C\x07epoch 3/3 done\r\n\x1b]133;D;0\x07\x1b]133;A\x07% ",
        base64(text.as_bytes())
    );
    h.fake.output(session.id, run.as_bytes()).unwrap();
    wait_until(|| {
        h.store
            .history(Some(session.id), 10)
            .unwrap()
            .first()
            .and_then(|command| command.exit_code)
            == Some(0)
    })
    .await;
    assert_eq!(
        h.manager.session(session.id).unwrap().state,
        SessionState::Idle
    );

    // Reopen SQLite as a restarted daemon would.
    let reopened = Store::open(&h.data_dir.join("sentinel.sqlite3")).unwrap();
    let history = reopened.history(Some(session.id), 10).unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].text, text);
    let stored = reopened.session(session.id).unwrap().unwrap();
    assert_eq!(stored.origin, SessionOrigin::Watch);
    assert_eq!(stored.capture, CaptureLevel::Full);
    assert_eq!(stored.title.as_deref(), Some("Qwen Training"));
    assert_eq!(stored.pid, session.pid);
    assert_eq!(
        h.fake.input(session.id).unwrap(),
        format!("{text}\r").as_bytes()
    );
    cleanup(h, session.id);
}

#[tokio::test]
async fn two_attached_clients_see_output_and_detaching_leaves_the_session_running() {
    let h = harness();
    let session = h.manager.create(watch(None)).await.unwrap();
    let mut first = h.manager.attach(session.id).unwrap();
    let mut second = h.manager.attach(session.id).unwrap();

    h.fake.output(session.id, b"step 1\r\n").unwrap();
    for attachment in [&mut first, &mut second] {
        let bytes = tokio::time::timeout(Duration::from_millis(100), attachment.output.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(bytes, b"step 1\r\n");
    }

    drop(first);
    h.fake.output(session.id, b"step 2\r\n").unwrap();
    let bytes = tokio::time::timeout(Duration::from_millis(100), second.output.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(bytes, b"step 2\r\n");
    drop(second);

    let reattached = h.manager.attach(session.id).unwrap();
    let screen = String::from_utf8_lossy(&reattached.screen).to_string();
    assert!(screen.contains("step 1") && screen.contains("step 2"));
    assert_eq!(
        h.manager.session(session.id).unwrap().state,
        SessionState::Idle
    );
    cleanup(h, session.id);
}

#[tokio::test]
async fn initial_command_is_rejected_before_spawn_unless_watch_with_a_hooked_shell() {
    let h = harness();
    let mut ui = watch(Some(argv(&["make"])));
    ui.origin = SessionOrigin::Ui;
    let error = h.manager.create(ui).await.unwrap_err();
    assert!(error.to_string().contains("WATCH"), "{error}");

    let mut plain_sh = watch(Some(argv(&["make"])));
    plain_sh.shell = Some("/bin/sh".into());
    let error = h.manager.create(plain_sh).await.unwrap_err();
    assert!(error.to_string().contains("/bin/sh"), "{error}");

    let mut auto_wrap = watch(None);
    auto_wrap.origin = SessionOrigin::AutoWrap;
    let error = h.manager.create(auto_wrap).await.unwrap_err();
    assert!(error.to_string().contains("AUTO_WRAP"), "{error}");

    let long = watch(Some(argv(&["echo", &"x".repeat(2_000)])));
    let error = h.manager.create(long).await.unwrap_err();
    assert!(
        error.to_string().contains("rendered initial command"),
        "{error}"
    );

    assert!(h.manager.list().unwrap().is_empty());
    fs::remove_dir_all(h.data_dir).unwrap();
}
