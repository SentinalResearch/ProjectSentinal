//! Milestone 6.4: the skill file agents install, and `sentinal agent-state`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use clap::Parser;
use cli::{args::Cli, AGENT_SKILL};

#[tokio::test]
async fn agent_skill_prints_the_bundled_file_and_skill_is_an_alias() {
    for name in ["agent-skill", "skill"] {
        let cli = Cli::try_parse_from(["sentinal", name]).unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        cli::app::run(cli, &mut out, &mut err).await;
        assert_eq!(String::from_utf8(out).unwrap(), AGENT_SKILL);
        assert!(err.is_empty());
    }
    let file = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/AGENT_SKILL.md"
    ))
    .unwrap();
    assert_eq!(file, AGENT_SKILL);
}

#[test]
fn every_command_in_the_skill_parses() {
    let mut in_block = false;
    let mut checked = 0;
    for line in AGENT_SKILL.lines() {
        if line.starts_with("```") {
            in_block = !in_block;
        } else if in_block && line.starts_with("sentinal ") {
            // Drop a trailing comment and keep quoted details as one word.
            let line = line.split(" #").next().unwrap();
            let mut words: Vec<String> = Vec::new();
            for (index, part) in line.split('"').enumerate() {
                if index % 2 == 1 {
                    words.push(part.to_string());
                } else {
                    words.extend(part.split_whitespace().map(str::to_string));
                }
            }
            Cli::try_parse_from(&words).unwrap_or_else(|error| panic!("{line}: {error}"));
            checked += 1;
        }
    }
    assert!(checked >= 6, "only {checked} commands found");
}

#[tokio::test]
async fn agent_state_outside_a_session_prints_nothing_and_succeeds() {
    // The test process has no SENTINAL_SESSION_ID unless run inside Sentinal.
    if std::env::var_os("SENTINAL_SESSION_ID").is_some() {
        return;
    }
    let cli = Cli::try_parse_from([
        "sentinal",
        "agent-state",
        "--status",
        "working",
        "--detail",
        "x",
    ])
    .unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = cli::app::run(cli, &mut out, &mut err).await;
    assert_eq!(
        format!("{code:?}"),
        format!("{:?}", std::process::ExitCode::SUCCESS)
    );
    assert!(out.is_empty() && err.is_empty());
}

#[test]
fn an_unknown_status_is_a_usage_error() {
    let error =
        Cli::try_parse_from(["sentinal", "agent-state", "--status", "sleeping"]).unwrap_err();
    assert_eq!(error.exit_code(), 2);
}
