//! Milestone 5.S: one Machine's ranked search over its own store.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::search::{search, SearchRequest};
use protocol::{
    search::{SearchFilters, SearchOutcome, SearchScope, SearchTarget, MAX_HIT_TITLE_CHARS},
    Command, CommandId, Event, EventId, EventKind, MachineId, Recipe, Session, SessionId,
    SessionState, Timestamp,
};
use store::Store;

const NOW: i64 = 10 * 86_400_000;
const HOUR: i64 = 3_600_000;

fn session(id: u64, title: &str, state: SessionState, at: i64) -> Session {
    let mut session = Session::new(
        SessionId(id),
        MachineId("rtx".into()),
        "/bin/zsh",
        "/home/me/ml",
        Timestamp(at),
    );
    session.title = Some(title.into());
    session
        .transition(SessionState::Idle, Timestamp(at))
        .unwrap();
    match state {
        SessionState::Idle => {}
        SessionState::Waiting => {
            session
                .transition(SessionState::Running, Timestamp(at))
                .unwrap();
            session
                .transition(SessionState::Waiting, Timestamp(at))
                .unwrap();
        }
        SessionState::Failed => {
            session
                .transition(SessionState::Running, Timestamp(at))
                .unwrap();
            session
                .transition(SessionState::Failed, Timestamp(at))
                .unwrap();
        }
        other => panic!("test helper does not build {other:?}"),
    }
    session
}

fn command(id: u64, text: &str, at: i64, exit: Option<i32>) -> Command {
    let mut command = Command::start(
        CommandId(id),
        SessionId(1),
        MachineId("rtx".into()),
        text,
        "/home/me/ml",
        Timestamp(at),
    );
    if let Some(code) = exit {
        command.finished_at = Some(Timestamp(at + 1));
        command.exit_code = Some(code);
    }
    command
}

fn error_event(id: u64, message: &str, at: i64) -> Event {
    Event {
        id: EventId(id),
        session_id: SessionId(1),
        command_id: Some(CommandId(1)),
        at: Timestamp(at),
        kind: EventKind::ErrorDetected {
            message: message.into(),
        },
    }
}

fn recipe(id: &str, name: &str, command: &str) -> Recipe {
    Recipe {
        id: id.into(),
        name: name.into(),
        category: None,
        project: None,
        machine_id: None,
        command: command.into(),
        params: Vec::new(),
    }
}

/// A training Machine: an out-of-memory run, a later good run, and an eval recipe.
fn fixture() -> (Store, Vec<Recipe>) {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_session(&session(
            1,
            "train moe",
            SessionState::Failed,
            NOW - 2 * HOUR,
        ))
        .unwrap();
    store
        .insert_session(&session(
            2,
            "eval run",
            SessionState::Waiting,
            NOW - 48 * HOUR,
        ))
        .unwrap();
    store
        .insert_command(&command(
            1,
            "python train.py --config moe.yaml",
            NOW - 2 * HOUR,
            Some(1),
        ))
        .unwrap();
    store
        .insert_command(&command(2, "python train.py", NOW - HOUR, Some(0)))
        .unwrap();
    store
        .insert_command(&command(3, "nvidia-smi", NOW - HOUR, None))
        .unwrap();
    store
        .insert_event(&error_event(
            1,
            "torch.OutOfMemoryError: CUDA out of memory",
            NOW - 2 * HOUR,
        ))
        .unwrap();
    (store, vec![recipe("eval", "Eval", "python eval.py")])
}

fn request(query: &str) -> SearchRequest<'_> {
    SearchRequest {
        query,
        scopes: &[],
        filters: SearchFilters::default(),
        limit: 20,
    }
}

fn titles(hits: &[protocol::search::SearchHit]) -> Vec<&str> {
    hits.iter().map(|hit| hit.title.as_str()).collect()
}

#[test]
fn oom_finds_the_out_of_memory_error_with_its_session() {
    let (store, recipes) = fixture();
    let hits = search(&store, &recipes, &request("oom"), Timestamp(NOW)).unwrap();
    // Initials of "out of memory" beat the loose o…o…m in the training Command.
    assert_eq!(hits.len(), 2, "{hits:?}");
    assert_eq!(hits[1].kind, SearchScope::Commands);
    assert_eq!(hits[0].kind, SearchScope::Events);
    assert_eq!(hits[0].title, "Error · train moe");
    assert_eq!(hits[0].detail, "torch.OutOfMemoryError: CUDA out of memory");
    assert_eq!(
        hits[0].target,
        SearchTarget::Event {
            session_id: SessionId(1),
            event_id: EventId(1)
        }
    );
}

#[test]
fn hits_are_ranked_best_first_across_kinds() {
    let (store, recipes) = fixture();
    let hits = search(&store, &recipes, &request("train"), Timestamp(NOW)).unwrap();
    // Session title "train moe" is a prefix match; both Commands a word start,
    // the newer one first.
    assert_eq!(
        titles(&hits),
        vec![
            "train moe",
            "python train.py",
            "python train.py --config moe.yaml"
        ]
    );
    assert!(hits.windows(2).all(|pair| pair[0].score >= pair[1].score));
    assert_eq!(hits[1].detail, "exit 0 · /home/me/ml");
    assert_eq!(
        hits[1].target,
        SearchTarget::Command {
            session_id: SessionId(1),
            command_id: CommandId(2)
        }
    );
}

#[test]
fn a_session_waiting_for_you_is_lifted() {
    let (store, recipes) = fixture();
    let hits = search(&store, &recipes, &request("eval"), Timestamp(NOW)).unwrap();
    // "eval run" is old but waiting; the recipe has no time and no boost.
    assert_eq!(titles(&hits), vec!["eval run", "Eval"]);
    assert_eq!(hits[0].detail, "Waiting · /home/me/ml");
    assert_eq!(
        hits[1].target,
        SearchTarget::Recipe {
            recipe_id: "eval".into()
        }
    );
}

#[test]
fn failed_narrows_to_failures_and_leaves_recipes_out() {
    let (store, recipes) = fixture();
    let failed = SearchRequest {
        filters: SearchFilters {
            since: None,
            outcome: Some(SearchOutcome::Failed),
        },
        ..request("")
    };
    let hits = search(&store, &recipes, &failed, Timestamp(NOW)).unwrap();
    let mut kinds: Vec<_> = hits
        .iter()
        .map(|hit| (hit.kind, hit.title.as_str()))
        .collect();
    kinds.sort();
    assert_eq!(
        kinds,
        vec![
            (SearchScope::Sessions, "train moe"),
            (SearchScope::Commands, "python train.py --config moe.yaml"),
            (SearchScope::Events, "Error · train moe"),
        ]
    );
    let running = SearchRequest {
        filters: SearchFilters {
            since: None,
            outcome: Some(SearchOutcome::Running),
        },
        scopes: &[SearchScope::Commands],
        ..request("")
    };
    assert_eq!(
        titles(&search(&store, &recipes, &running, Timestamp(NOW)).unwrap()),
        vec!["nvidia-smi"]
    );
}

#[test]
fn since_scopes_and_limit_narrow_the_hits() {
    let (store, recipes) = fixture();
    let recent = SearchRequest {
        filters: SearchFilters {
            since: Some(Timestamp(NOW - 90 * 60_000)),
            outcome: None,
        },
        ..request("python")
    };
    assert_eq!(
        titles(&search(&store, &recipes, &recent, Timestamp(NOW)).unwrap()),
        vec!["python train.py"]
    );
    let recipes_only = SearchRequest {
        scopes: &[SearchScope::Recipes],
        ..request("python")
    };
    assert_eq!(
        titles(&search(&store, &recipes, &recipes_only, Timestamp(NOW)).unwrap()),
        vec!["Eval"]
    );
    let one = SearchRequest {
        limit: 1,
        ..request("python")
    };
    assert_eq!(
        search(&store, &recipes, &one, Timestamp(NOW))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn long_text_is_clipped_to_one_line() {
    let store = Store::open_in_memory().unwrap();
    let long = format!("echo {}\nsecond line", "x".repeat(400));
    store
        .insert_command(&command(1, &long, NOW, Some(0)))
        .unwrap();
    let hits = search(&store, &[], &request("echo"), Timestamp(NOW)).unwrap();
    assert_eq!(hits[0].title.chars().count(), MAX_HIT_TITLE_CHARS);
    assert!(hits[0].title.ends_with('…'));
    assert!(!hits[0].title.contains('\n'));
}

mod over_the_api {
    use super::*;
    use daemon::{
        api::{AppState, WsAction},
        audit::AuditLog,
        auth::AuthManager,
        pty::FakePty,
        session_manager::{SessionManager, SessionManagerConfig},
    };
    use protocol::{
        search::MAX_SEARCH_LIMIT, ClientRequest, DaemonMessage, Envelope, Machine, MachineStatus,
        Os,
    };
    use std::sync::Arc;

    fn state(store: Arc<Store>) -> AppState {
        let data_dir = std::env::temp_dir().join(format!(
            "sentinal-search-api-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let manager = SessionManager::with_store(
            Arc::new(FakePty::new()),
            SessionManagerConfig {
                machine_id: MachineId("rtx".into()),
                data_dir: data_dir.clone(),
                scrollback_lines: 100,
                clock: Arc::new(|| Timestamp(NOW)),
            },
            AuditLog::new(&data_dir),
            store,
        )
        .unwrap();
        AppState::new(
            manager,
            Machine {
                id: MachineId("rtx".into()),
                name: "RTX".into(),
                os: Os::Linux,
                cpu: "cpu".into(),
                cpu_cores: 8,
                ram_bytes: 1,
                gpus: vec![],
                status: MachineStatus::Online,
                last_seen: Timestamp(NOW),
                role: None,
            },
            AuthManager::load(&data_dir.join("auth.json"), "token".into()).unwrap(),
        )
    }

    async fn send(state: &AppState, request: ClientRequest) -> WsAction {
        state
            .dispatch_text(&Envelope::new(request).encode().unwrap())
            .await
    }

    #[tokio::test]
    async fn search_replies_with_ranked_hits_and_refuses_bad_limits() {
        let (store, _) = fixture();
        let state = state(Arc::new(store));
        let reply = send(
            &state,
            ClientRequest::Search {
                query: "oom".into(),
                scopes: Vec::new(),
                filters: SearchFilters::default(),
                limit: 10,
            },
        )
        .await;
        assert!(matches!(
            reply,
            WsAction::Reply(DaemonMessage::SearchResults { ref hits })
                if hits.first().is_some_and(|hit| hit.kind == SearchScope::Events)
        ));
        let refused = send(
            &state,
            ClientRequest::Search {
                query: "oom".into(),
                scopes: Vec::new(),
                filters: SearchFilters::default(),
                limit: MAX_SEARCH_LIMIT + 1,
            },
        )
        .await;
        assert!(matches!(
            refused,
            WsAction::Reply(DaemonMessage::Error { .. })
        ));
    }
}

/// Budget (5.S): p95 under 50 ms on 100 000 Commands and 500 000 Events.
/// Run with `cargo test --release -p daemon --test search -- --ignored`.
#[test]
#[ignore = "timed; run in release"]
fn search_meets_its_budget_on_a_large_history() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_session(&session(1, "train moe", SessionState::Failed, NOW))
        .unwrap();
    let words = [
        "python", "cargo", "git", "npm", "ls", "docker", "pytest", "make",
    ];
    let args = [
        "train.py", "build", "status", "install", "-la", "run", "tests/", "all",
    ];
    {
        for id in 1..=100_000_u64 {
            let text = format!(
                "{} {} --seed {id}",
                words[id as usize % words.len()],
                args[(id as usize / 7) % args.len()]
            );
            store
                .insert_command(&command(
                    id,
                    &text,
                    NOW - 100_000 + id as i64,
                    Some((id % 5 == 0) as i32),
                ))
                .unwrap();
        }
        for id in 1..=500_000_u64 {
            let kind = if id % 50 == 0 {
                EventKind::ErrorDetected {
                    message: format!("RuntimeError: step {id} failed"),
                }
            } else {
                EventKind::ProgressUpdated {
                    current: id,
                    total: Some(500_000),
                    unit: "step".into(),
                }
            };
            store
                .insert_event(&Event {
                    id: EventId(id),
                    session_id: SessionId(1),
                    command_id: None,
                    at: Timestamp(NOW - 500_000 + id as i64),
                    kind,
                })
                .unwrap();
        }
    }
    let recipes = vec![recipe("eval", "Eval", "python eval.py")];
    let queries = [
        "py",
        "train",
        "oom",
        "cargo build",
        "runtimeerror",
        "seed 4242",
        "xyz",
        "tests",
    ];
    let mut timings = Vec::new();
    for round in 0..5 {
        for query in queries {
            let started = std::time::Instant::now();
            let hits = search(&store, &recipes, &request(query), Timestamp(NOW)).unwrap();
            timings.push(started.elapsed());
            if round == 0 {
                eprintln!(
                    "{query:>14}: {:>3} hits in {:?}",
                    hits.len(),
                    started.elapsed()
                );
            }
        }
    }
    timings.sort();
    let p95 = timings[timings.len() * 95 / 100];
    eprintln!("p95 {p95:?} over {} searches", timings.len());
    assert!(p95 < std::time::Duration::from_millis(50), "p95 {p95:?}");
}
