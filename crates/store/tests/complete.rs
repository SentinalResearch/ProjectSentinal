//! Milestone 5.A: per-Command-text history for completion.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{complete::CandidateStats, Command, CommandId, MachineId, SessionId, Timestamp};
use store::{CompletionQuery, Store};

fn run(store: &Store, id: u64, text: &str, cwd: &str, at: i64, exit: Option<i32>) {
    let mut command = Command::start(
        CommandId(id),
        SessionId(1),
        MachineId("m".into()),
        text,
        cwd,
        Timestamp(at),
    );
    store.insert_command(&command).unwrap();
    if let Some(code) = exit {
        command.finished_at = Some(Timestamp(at + 1));
        command.exit_code = Some(code);
        store.finish_command(&command).unwrap();
    }
}

fn query<'a>(prefix: &'a str) -> CompletionQuery<'a> {
    CompletionQuery {
        prefix,
        cwd: Some("/home/me/ml"),
        project_root: Some("/home/me/ml"),
    }
}

#[test]
fn history_is_grouped_by_text_under_the_prefix() {
    let store = Store::open_in_memory().unwrap();
    run(&store, 1, "python train.py", "/home/me/ml", 10, Some(0));
    run(&store, 2, "python train.py", "/home/me/ml/sub", 20, Some(0));
    run(&store, 3, "python train.py", "/tmp", 30, Some(1));
    run(&store, 4, "python train.py", "/tmp", 40, None);
    run(&store, 5, "python eval.py", "/home/me/ml", 50, Some(0));
    run(&store, 6, "pytest", "/home/me/ml", 60, Some(0));
    run(&store, 7, "Python other", "/home/me/ml", 70, Some(0));

    let rows = store.completion_candidates(&query("python tr")).unwrap();
    assert_eq!(rows.len(), 1);
    let (text, stats) = &rows[0];
    assert_eq!(text, "python train.py");
    assert_eq!(
        *stats,
        CandidateStats {
            succeeded: 2,
            failed: 1,
            last_at: Timestamp(40),
            in_project: 2,
            in_cwd: 1,
        }
    );
    // The latest run is still going: the last *finished* run exited 1.
    assert_eq!(
        store.last_finished_exit("python train.py").unwrap(),
        Some(1)
    );
    assert_eq!(store.last_finished_exit("never ran").unwrap(), None);

    // Case matters, as in the shell.
    let texts: Vec<String> = store
        .completion_candidates(&query("python"))
        .unwrap()
        .into_iter()
        .map(|row| row.0)
        .collect();
    assert_eq!(texts.len(), 2);
    assert!(!texts.contains(&"Python other".to_string()));
}

#[test]
fn a_sibling_directory_is_not_inside_the_project() {
    let store = Store::open_in_memory().unwrap();
    run(&store, 1, "make", "/home/me/ml-old", 10, Some(0));
    run(&store, 2, "make", "/home/me/ml", 20, Some(0));
    let rows = store.completion_candidates(&query("ma")).unwrap();
    assert_eq!(rows[0].1.in_project, 1);
}

#[test]
fn exact_and_multi_line_text_is_not_offered() {
    let store = Store::open_in_memory().unwrap();
    run(&store, 1, "ls", "/", 10, Some(0));
    run(&store, 2, "ls -la", "/", 20, Some(0));
    run(&store, 3, "ls <<EOF\nx\nEOF", "/", 30, Some(0));
    let texts: Vec<String> = store
        .completion_candidates(&query("ls"))
        .unwrap()
        .into_iter()
        .map(|row| row.0)
        .collect();
    assert_eq!(texts, vec!["ls -la"]);
}

#[test]
fn prefixes_with_like_wildcards_or_odd_characters_match_literally() {
    let store = Store::open_in_memory().unwrap();
    run(&store, 1, "echo 100%_done", "/", 10, Some(0));
    run(&store, 2, "echo 100xydone", "/", 20, Some(0));
    run(&store, 3, "grep 'é' file", "/", 30, Some(0));
    let texts = |prefix: &str| -> Vec<String> {
        store
            .completion_candidates(&query(prefix))
            .unwrap()
            .into_iter()
            .map(|row| row.0)
            .collect()
    };
    assert_eq!(texts("echo 100%_"), vec!["echo 100%_done"]);
    assert_eq!(texts("grep 'é"), vec!["grep 'é' file"]);
}

#[test]
fn an_empty_prefix_offers_recent_history_without_cwd_or_project() {
    let store = Store::open_in_memory().unwrap();
    run(&store, 1, "git status", "/repo", 10, Some(0));
    let rows = store
        .completion_candidates(&CompletionQuery {
            prefix: "",
            cwd: None,
            project_root: None,
        })
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].1.in_cwd, 0);
    assert_eq!(rows[0].1.in_project, 0);
}
