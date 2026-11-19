//! A real prompt leaves the cursor on its line: no newline ever arrives. It must
//! still make the Session WAITING, through the same pipeline a terminal uses.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::{
    audit::AuditLog,
    pty::FakePty,
    session_manager::{CreateSession, SessionManager, SessionManagerConfig},
};
use protocol::{
    EventKind, MachineId, ProcessTotals, SessionId, SessionMetrics, SessionOrigin, SessionState,
    Timestamp,
};
use std::{
    sync::{
        atomic::{AtomicI64, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use store::Store;

const START: i64 = 1_790_000_000_000;

struct Harness {
    manager: SessionManager,
    fake: Arc<FakePty>,
    clock: Arc<AtomicI64>,
    started: Mutex<Vec<SessionId>>,
}

/// A fake terminal's reader blocks until it exits, and the runtime waits for
/// that reader, so a failed assertion would otherwise hang instead of report.
impl Drop for Harness {
    fn drop(&mut self) {
        for id in self.started.lock().unwrap().drain(..) {
            let _ = self.fake.exit(id, 0);
        }
    }
}

fn harness(name: &str) -> Harness {
    let dir = std::env::temp_dir().join(format!("sentinal-prompt-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fake = Arc::new(FakePty::new());
    let clock = Arc::new(AtomicI64::new(START));
    let now = clock.clone();
    let manager = SessionManager::with_store(
        fake.clone(),
        SessionManagerConfig {
            machine_id: MachineId("m".into()),
            data_dir: dir.clone(),
            scrollback_lines: 100,
            clock: Arc::new(move || Timestamp(now.load(Ordering::SeqCst))),
        },
        AuditLog::new(&dir),
        Arc::new(Store::open_in_memory().unwrap()),
    )
    .unwrap();
    Harness {
        manager,
        fake,
        clock,
        started: Mutex::new(Vec::new()),
    }
}

impl Harness {
    fn signal(&self, id: SessionId, payload: &str) -> String {
        let request = self.fake.spawn_request(id).unwrap();
        let (_, nonce) = request
            .env
            .iter()
            .find(|(key, _)| key == "SENTINAL_OSC_NONCE")
            .unwrap();
        format!("\x1b]{payload};sentinal={nonce}\x07")
    }

    /// A running Command, as the shell hook reports it.
    async fn running_command(&self) -> SessionId {
        let id = self
            .manager
            .create(CreateSession {
                shell: Some("/bin/zsh".into()),
                cwd: Some("/work".into()),
                cols: 80,
                rows: 24,
                origin: SessionOrigin::Watch,
                title: None,
                command: None,
                env: None,
                sandbox: false,
            })
            .await
            .unwrap()
            .id;
        self.started.lock().unwrap().push(id);
        self.fake
            .output(id, format!("{}% ", self.signal(id, "133;A")).as_bytes())
            .unwrap();
        // Enter ends the shell's prompt line.
        self.fake.output(id, b"\r\n").unwrap();
        // `python job.py` is running.
        let started = [
            self.signal(id, "7420;cmd=cHl0aG9uIGpvYi5weQ=="),
            self.signal(id, "133;B"),
        ]
        .concat();
        self.fake.output(id, started.as_bytes()).unwrap();
        self.wait_for(id, SessionState::Running).await;
        id
    }

    async fn wait_for(&self, id: SessionId, state: SessionState) {
        for _ in 0..200 {
            if self.manager.session(id).unwrap().state == state {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        panic!(
            "Session stayed {:?}",
            self.manager.session(id).unwrap().state
        );
    }

    /// Time passes with no output, and a metrics sample notices.
    fn quiet_for(&self, id: SessionId, ms: i64) {
        let at = self.clock.fetch_add(ms, Ordering::SeqCst) + ms;
        self.manager
            .publish_metrics(SessionMetrics {
                session_id: id,
                at: Timestamp(at),
                totals: ProcessTotals {
                    cpu_percent: 0.0,
                    rss_bytes: 1,
                    gpu_mem_bytes: 0,
                },
            })
            .unwrap();
    }
}

#[tokio::test]
async fn a_prompt_with_no_newline_makes_the_session_waiting_and_an_answer_resumes_it() {
    let h = harness("waits");
    let id = h.running_command().await;

    h.fake.output(id, b"Publish the results? [y/N] ").unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    h.quiet_for(id, 3_000);
    h.wait_for(id, SessionState::Waiting).await;

    // The owner's browser fetches exactly this Event for the words.
    let events = h.manager.events(Some(id), None, 100).unwrap();
    let asked = events
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::WaitingForInput { prompt, .. } => prompt.clone(),
            _ => None,
        })
        .unwrap();
    assert_eq!(asked, "Publish the results? [y/N]");

    // The terminal echoes the answer and Enter: the program is working again.
    h.fake.output(id, b"y\r\n").unwrap();
    h.wait_for(id, SessionState::Running).await;
}

#[tokio::test]
async fn ordinary_unfinished_output_is_not_a_question() {
    let h = harness("output");
    let id = h.running_command().await;

    h.fake.output(id, b"Downloading 40%").unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    h.quiet_for(id, 3_000);
    tokio::time::sleep(Duration::from_millis(40)).await;
    assert_eq!(h.manager.session(id).unwrap().state, SessionState::Running);
}

#[tokio::test]
async fn a_full_screen_agent_s_menu_is_read_from_the_screen_with_its_words_intact() {
    // Real agents move the cursor instead of printing spaces, which is why the
    // line stream read "Doyouwanttoproceed?" and lost the question.
    let h = harness("screen");
    let id = h.running_command().await;
    h.fake
        .output(
            id,
            "\x1b[?1049h\x1b[2J\x1b[H\
             Do\x1b[1Cyou\x1b[1Cwant\x1b[1Cto\x1b[1Cproceed?\r\n\
             \u{276f} 1.\x1b[1CYes\r\n\
             \x20\x202.\x1b[1CNo\r\n\
             Esc\x1b[1Cto\x1b[1Ccancel\r\n"
                .as_bytes(),
        )
        .unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    h.quiet_for(id, 1_000);
    h.quiet_for(id, 1_000);
    h.wait_for(id, SessionState::Waiting).await;
    let events = h.manager.events(Some(id), None, 100).unwrap();
    let (prompt, options) = events
        .iter()
        .find_map(|event| match &event.kind {
            EventKind::WaitingForInput {
                prompt, options, ..
            } => Some((prompt.clone().unwrap(), options.clone())),
            _ => None,
        })
        .unwrap();
    assert_eq!(prompt, "Do you want to proceed?");
    assert_eq!(options.len(), 2);

    // Its status line keeps redrawing, and the question stays.
    for second in 0..30 {
        h.fake
            .output(
                id,
                format!("\x1b[10;1Hworking {second}\r\nmore\r\n").as_bytes(),
            )
            .unwrap();
        tokio::time::sleep(Duration::from_millis(2)).await;
        h.quiet_for(id, 1_000);
    }
    assert_eq!(h.manager.session(id).unwrap().state, SessionState::Waiting);

    // Answered: the menu leaves the screen, and the Session is working again.
    h.fake.output(id, b"\x1b[2J\x1b[Hdone\r\n").unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    h.quiet_for(id, 1_100);
    h.quiet_for(id, 1_100);
    h.wait_for(id, SessionState::Running).await;
}
