//! Milestone 5.S: search candidates from the trigram and prefix indexes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{Command, CommandId, Event, EventId, EventKind, MachineId, SessionId, Timestamp};
use store::{SearchCandidateQuery, Store, RECENT_SEARCH_WINDOW};

fn command(id: u64, text: &str, at: i64) -> Command {
    Command::start(
        CommandId(id),
        SessionId(1),
        MachineId("test-machine".into()),
        text,
        "/work",
        Timestamp(at),
    )
}

fn event(id: u64, kind: EventKind, at: i64) -> Event {
    Event {
        id: EventId(id),
        session_id: SessionId(1),
        command_id: None,
        at: Timestamp(at),
        kind,
    }
}

fn error(message: &str) -> EventKind {
    EventKind::ErrorDetected {
        message: message.into(),
    }
}

fn query(text: &str) -> SearchCandidateQuery<'_> {
    SearchCandidateQuery {
        text,
        since: None,
        commands: true,
        events: true,
    }
}

fn command_texts(store: &Store, text: &str) -> Vec<String> {
    store
        .search_candidates(&query(text))
        .unwrap()
        .commands
        .into_iter()
        .map(|command| command.text)
        .collect()
}

#[test]
fn the_schema_has_the_search_indexes() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(store.schema_version().unwrap(), 12);
    for table in ["commands_trigram", "events_trigram"] {
        assert!(store.has_table(table).unwrap(), "missing {table}");
    }
}

#[test]
fn a_substring_is_found_far_beyond_the_recent_window() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_command(&command(1, "python train.py --config moe.yaml", 1))
        .unwrap();
    let filler = RECENT_SEARCH_WINDOW as u64 + 200;
    for id in 2..2 + filler {
        store
            .insert_command(&command(id, &format!("ls -la dir{id}"), id as i64))
            .unwrap();
    }
    // "rain" starts no word: only the trigram index finds it.
    assert_eq!(
        command_texts(&store, "rain"),
        vec!["python train.py --config moe.yaml"]
    );
    // Every word must be present.
    assert_eq!(
        command_texts(&store, "moe config"),
        vec!["python train.py --config moe.yaml"]
    );
    assert!(command_texts(&store, "moe nothing-like-this").is_empty());
}

#[test]
fn short_words_use_the_prefix_index() {
    let store = Store::open_in_memory().unwrap();
    store.insert_command(&command(1, "python a.py", 1)).unwrap();
    for id in 2..2 + RECENT_SEARCH_WINDOW as u64 + 10 {
        store.insert_command(&command(id, "ls", id as i64)).unwrap();
    }
    assert_eq!(command_texts(&store, "py"), vec!["python a.py"]);
}

#[test]
fn edited_command_text_is_reindexed() {
    let store = Store::open_in_memory().unwrap();
    let mut started = command(1, "echo draft", 1);
    store.insert_command(&started).unwrap();
    started.text = "echo final".into();
    started.finished_at = Some(Timestamp(2));
    started.exit_code = Some(0);
    store.finish_command(&started).unwrap();
    for id in 2..2 + RECENT_SEARCH_WINDOW as u64 + 10 {
        store.insert_command(&command(id, "ls", id as i64)).unwrap();
    }
    assert_eq!(command_texts(&store, "final"), vec!["echo final"]);
    assert!(command_texts(&store, "draft").is_empty());
}

#[test]
fn text_bearing_events_are_indexed_and_others_are_not() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_event(&event(
            1,
            error("torch.OutOfMemoryError: CUDA out of memory"),
            1,
        ))
        .unwrap();
    store
        .insert_event(&event(
            2,
            EventKind::WaitingForInput {
                prompt: Some("Continue? [y/N]".into()),
                options: Vec::new(),
            },
            2,
        ))
        .unwrap();
    store
        .insert_event(&event(
            3,
            EventKind::CommandStarted {
                text: "python memory_test.py".into(),
            },
            3,
        ))
        .unwrap();
    for id in 4..4 + 20_000 {
        store
            .insert_event(&event(
                id,
                EventKind::ProgressUpdated {
                    current: id,
                    total: None,
                    unit: "step".into(),
                },
                id as i64,
            ))
            .unwrap();
    }
    let ids = |text: &str| -> Vec<u64> {
        store
            .search_candidates(&query(text))
            .unwrap()
            .events
            .into_iter()
            .map(|event| event.id.0)
            .collect()
    };
    assert_eq!(ids("memory"), vec![1]);
    assert_eq!(ids("continue"), vec![2]);
    // Command text is searched as a Command, not again as its start Event.
    assert!(ids("memory_test").is_empty());
}

#[test]
fn recent_rows_are_candidates_for_fuzzy_matches() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_event(&event(1, error("CUDA out of memory"), 1))
        .unwrap();
    store.insert_command(&command(1, "nvidia-smi", 1)).unwrap();
    // No index holds "oom" or "nvsmi"; recent rows are matched as subsequences.
    let found = store.search_candidates(&query("oom")).unwrap();
    assert_eq!(found.events.len(), 1);
    assert!(found.commands.is_empty());
    assert_eq!(command_texts(&store, "nvsmi"), vec!["nvidia-smi"]);
}

#[test]
fn since_and_scopes_narrow_the_candidates() {
    let store = Store::open_in_memory().unwrap();
    store.insert_command(&command(1, "make old", 100)).unwrap();
    store.insert_command(&command(2, "make new", 200)).unwrap();
    store
        .insert_event(&event(1, error("make failed"), 150))
        .unwrap();
    let recent = store
        .search_candidates(&SearchCandidateQuery {
            text: "make",
            since: Some(Timestamp(150)),
            commands: true,
            events: true,
        })
        .unwrap();
    assert_eq!(
        recent
            .commands
            .iter()
            .map(|command| command.text.as_str())
            .collect::<Vec<_>>(),
        vec!["make new"]
    );
    assert_eq!(recent.events.len(), 1);
    let commands_only = store
        .search_candidates(&SearchCandidateQuery {
            text: "make",
            since: None,
            commands: true,
            events: false,
        })
        .unwrap();
    assert_eq!(commands_only.commands.len(), 2);
    assert!(commands_only.events.is_empty());
}

#[test]
fn query_syntax_in_the_text_is_never_interpreted() {
    let store = Store::open_in_memory().unwrap();
    store
        .insert_command(&command(1, "echo \"a OR b\" NEAR(x)", 1))
        .unwrap();
    for text in [
        "\"a OR", "NEAR(x)", "a* OR", "-x", "col:val", "\"\"\"", "^ab",
    ] {
        assert!(store.search_candidates(&query(text)).is_ok(), "{text}");
    }
    assert_eq!(command_texts(&store, "NEAR(x)").len(), 1);
}
