//! Milestone 5.A: ranked completion on one Machine.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use daemon::complete::{complete, CompleteRequest};
use protocol::{
    complete::{CompletionSource, CompletionWeights},
    Command, CommandId, MachineId, Recipe, RecipeParam, SessionId, Timestamp,
};
use store::Store;

const DAY: i64 = 86_400_000;
const NOW: i64 = 100 * DAY;

fn run(store: &Store, id: u64, text: &str, cwd: &str, days_ago: i64, exit: i32) {
    let at = NOW - days_ago * DAY - id as i64;
    let mut command = Command::start(
        CommandId(id),
        SessionId(1),
        MachineId("m".into()),
        text,
        cwd,
        Timestamp(at),
    );
    store.insert_command(&command).unwrap();
    command.finished_at = Some(Timestamp(at + 1));
    command.exit_code = Some(exit);
    store.finish_command(&command).unwrap();
}

fn recipe(id: &str, command: &str, params: Vec<RecipeParam>) -> Recipe {
    Recipe {
        id: id.into(),
        name: id.into(),
        category: None,
        project: None,
        machine_id: None,
        command: command.into(),
        params,
    }
}

fn request(prefix: &str) -> CompleteRequest<'_> {
    CompleteRequest {
        prefix,
        cwd: Some("/home/me/ml"),
        project_root: Some("/home/me/ml"),
        include_failed: false,
        limit: 10,
    }
}

fn texts(store: &Store, recipes: &[Recipe], request: &CompleteRequest<'_>) -> Vec<String> {
    complete(
        store,
        recipes,
        &CompletionWeights::default(),
        request,
        Timestamp(NOW),
    )
    .unwrap()
    .into_iter()
    .map(|completion| completion.text)
    .collect()
}

/// The spec example: `python tr` → `python train.py` (31×), then with
/// `--config`, then the recipe.
#[test]
fn the_spec_example_ranks_history_by_frecency_then_recipes() {
    let store = Store::open_in_memory().unwrap();
    let mut id = 0;
    for day in 0..31 {
        id += 1;
        run(&store, id, "python train.py", "/home/me/ml", day % 10, 0);
    }
    for _ in 0..6 {
        id += 1;
        run(
            &store,
            id,
            "python train.py --config moe.yaml",
            "/home/me/ml",
            0,
            0,
        );
    }
    let recipes = vec![recipe(
        "train-big",
        "python train.py --config {config}",
        vec![RecipeParam::with_default("config", "big.yaml")],
    )];
    let ranked = complete(
        &store,
        &recipes,
        &CompletionWeights::default(),
        &request("python tr"),
        Timestamp(NOW),
    )
    .unwrap();
    assert_eq!(
        ranked.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        vec![
            "python train.py",
            "python train.py --config moe.yaml",
            "python train.py --config big.yaml"
        ]
    );
    assert_eq!(ranked[0].count, 31);
    assert_eq!(ranked[0].source, CompletionSource::History);
    assert_eq!(ranked[0].last_exit, Some(0));
    assert_eq!(ranked[2].source, CompletionSource::Recipe);
    assert_eq!(ranked[2].last_at, None);
}

#[test]
fn failed_commands_are_left_out_unless_asked_for() {
    let store = Store::open_in_memory().unwrap();
    run(&store, 1, "cargo test", "/home/me/ml", 0, 101);
    run(&store, 2, "cargo build", "/home/me/ml", 0, 0);
    assert_eq!(texts(&store, &[], &request("cargo")), vec!["cargo build"]);
    let with_failures = CompleteRequest {
        include_failed: true,
        ..request("cargo")
    };
    assert_eq!(texts(&store, &[], &with_failures).len(), 2);
}

#[test]
fn the_current_project_and_directory_win_a_tie() {
    let store = Store::open_in_memory().unwrap();
    run(&store, 1, "make deploy", "/srv/other", 0, 0);
    run(&store, 2, "make test", "/home/me/ml/src", 0, 0);
    run(&store, 3, "make all", "/home/me/ml", 0, 0);
    assert_eq!(
        texts(&store, &[], &request("make")),
        vec!["make all", "make test", "make deploy"]
    );
}

#[test]
fn a_recipe_with_a_required_value_completes_up_to_it() {
    let store = Store::open_in_memory().unwrap();
    let recipes = vec![
        recipe(
            "serve",
            "vllm serve {model} --port {port}",
            vec![
                RecipeParam::required("model"),
                RecipeParam::with_default("port", "8000"),
            ],
        ),
        recipe("unrelated", "ls -la", vec![]),
    ];
    assert_eq!(
        texts(&store, &recipes, &request("vllm")),
        vec!["vllm serve "]
    );
    assert!(texts(&store, &recipes, &request("vllm serve ")).is_empty());
}

#[test]
fn the_limit_is_respected_and_history_never_repeats_a_recipe() {
    let store = Store::open_in_memory().unwrap();
    for id in 1..=30 {
        run(&store, id, &format!("echo {id}"), "/", 0, 0);
    }
    run(&store, 31, "ls -la", "/", 0, 0);
    let limited = CompleteRequest {
        limit: 5,
        ..request("echo")
    };
    assert_eq!(texts(&store, &[], &limited).len(), 5);
    let recipes = vec![recipe("ls", "ls -la", vec![])];
    let ranked = texts(&store, &recipes, &request("ls"));
    assert_eq!(ranked, vec!["ls -la"]);
}

/// Budget (5.A): p95 under 20 ms on 100 000 history rows.
/// Run with `cargo test --release -p daemon --test complete -- --ignored`.
#[test]
#[ignore = "timed; run in release"]
fn completion_meets_its_budget_on_a_large_history() {
    let store = Store::open_in_memory().unwrap();
    let words = [
        "python", "cargo", "git", "npm", "ls", "docker", "pytest", "make",
    ];
    let args = [
        "train.py", "build", "status", "install", "-la", "run", "tests/", "all",
    ];
    for id in 1..=100_000_u64 {
        let text = format!(
            "{} {} {}",
            words[id as usize % words.len()],
            args[(id as usize / 7) % args.len()],
            id % 3_000
        );
        let cwd = if id % 4 == 0 {
            "/home/me/ml"
        } else {
            "/srv/app"
        };
        run(
            &store,
            id,
            &text,
            cwd,
            (id % 60) as i64,
            (id % 9 == 0) as i32,
        );
    }
    let prefixes = [
        "",
        "p",
        "py",
        "python ",
        "python train",
        "git st",
        "cargo build 12",
        "zzz",
    ];
    let mut timings = Vec::new();
    for round in 0..5 {
        for prefix in prefixes {
            let started = std::time::Instant::now();
            let found = complete(
                &store,
                &[],
                &CompletionWeights::default(),
                &request(prefix),
                Timestamp(NOW),
            )
            .unwrap();
            timings.push(started.elapsed());
            if round == 0 {
                eprintln!(
                    "{prefix:>16}: {:>2} in {:?}",
                    found.len(),
                    started.elapsed()
                );
            }
        }
    }
    timings.sort();
    let p95 = timings[timings.len() * 95 / 100];
    eprintln!("p95 {p95:?} over {} completions", timings.len());
    assert!(p95 < std::time::Duration::from_millis(20), "p95 {p95:?}");
}

mod over_the_api {
    use super::*;
    use daemon::{
        api::{AppState, WsAction},
        audit::AuditLog,
        auth::AuthManager,
        complete::Completer,
        pty::FakePty,
        session_manager::{SessionManager, SessionManagerConfig},
    };
    use protocol::{ClientRequest, DaemonMessage, Envelope, Machine, MachineStatus, Os};
    use std::sync::Arc;

    #[tokio::test]
    async fn complete_replies_with_ranked_completions() {
        let store = Store::open_in_memory().unwrap();
        run(&store, 1, "git status", "/repo", 0, 0);
        run(&store, 2, "git push", "/repo", 3, 0);
        let data_dir =
            std::env::temp_dir().join(format!("sentinal-complete-api-{}", std::process::id()));
        let manager = SessionManager::with_store(
            Arc::new(FakePty::new()),
            SessionManagerConfig {
                machine_id: MachineId("m".into()),
                data_dir: data_dir.clone(),
                scrollback_lines: 100,
                clock: Arc::new(|| Timestamp(NOW)),
            },
            AuditLog::new(&data_dir),
            Arc::new(store),
        )
        .unwrap();
        let state = AppState::new(
            manager,
            Machine {
                id: MachineId("m".into()),
                name: "M".into(),
                os: Os::Linux,
                cpu: "cpu".into(),
                cpu_cores: 1,
                ram_bytes: 1,
                gpus: vec![],
                status: MachineStatus::Online,
                last_seen: Timestamp(NOW),
                role: None,
            },
            AuthManager::load(&data_dir.join("auth.json"), "token".into()).unwrap(),
        )
        .with_completer(Completer::new(CompletionWeights::default()));
        let reply = state
            .dispatch_text(
                &Envelope::new(ClientRequest::Complete {
                    session_id: None,
                    prefix: "git ".into(),
                    cwd: Some("/repo".into()),
                    include_failed: false,
                    limit: 5,
                })
                .encode()
                .unwrap(),
            )
            .await;
        let WsAction::Reply(DaemonMessage::Completions { completions }) = reply else {
            panic!("expected completions");
        };
        assert_eq!(
            completions
                .iter()
                .map(|c| c.text.as_str())
                .collect::<Vec<_>>(),
            vec!["git status", "git push"]
        );
        assert_eq!(completions[0].last_exit, Some(0));
    }
}
