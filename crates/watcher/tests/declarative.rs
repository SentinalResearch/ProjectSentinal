//! Declarative adapter contract tests.

use protocol::{Command, CommandId, EventKind, MachineId, SessionId, Timestamp};
use watcher::{DeclarativeAdapter, Observation, WatcherAdapter};

fn command(text: &str) -> Command {
    Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("machine".into()),
        text,
        "/work",
        Timestamp(0),
    )
}

#[test]
fn valid_declarative_rule_emits_progress() {
    let text = r#"
name = "my-tool"
match_command = "^mytool "
[[rule]]
regex = 'step (?P<current>\d+)/(?P<total>\d+)'
emit = "PROGRESS_UPDATED"
unit = "step"
"#;
    let mut adapter = DeclarativeAdapter::parse("my-tool.toml", text).unwrap();
    assert!(adapter.matches(&command("mytool run")));
    assert!(matches!(
        adapter
            .observe(&Observation::output(Timestamp(0), "step 4/10"))
            .as_slice(),
        [EventKind::ProgressUpdated { current: 4, total: Some(10), unit }] if unit == "step"
    ));
}

#[test]
fn invalid_regex_names_file_and_rule() {
    let text = r#"
name = "broken"
match_command = "^broken "
[[rule]]
regex = "(unclosed"
emit = "ERROR_DETECTED"
"#;
    let error = DeclarativeAdapter::parse("broken.toml", text).unwrap_err();
    assert!(error.to_string().contains("broken.toml"));
    assert!(error.to_string().contains("rule 1"));
}
