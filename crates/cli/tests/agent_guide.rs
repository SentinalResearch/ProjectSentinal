//! The guide agents are pointed to exists, is reachable, and tells the truth.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use clap::{CommandFactory, Parser};
use cli::{args::Cli, AGENT_GUIDE};

#[test]
fn help_points_agents_to_the_guide() {
    let help = Cli::command().render_help().to_string();
    assert!(help.contains("sentinal agent-guide"), "{help}");
}

#[tokio::test]
async fn the_command_prints_the_guide() {
    let cli = Cli::try_parse_from(["sentinal", "agent-guide"]).unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    cli::app::run(cli, &mut out, &mut err).await;
    assert_eq!(String::from_utf8(out).unwrap(), AGENT_GUIDE);
    assert!(err.is_empty());
}

#[test]
fn every_command_in_the_guide_parses() {
    let mut in_block = false;
    let mut checked = 0;
    for line in AGENT_GUIDE.lines() {
        if line.starts_with("```") {
            in_block = !in_block;
        } else if in_block && line.starts_with("sentinal ") {
            let words = line.split_whitespace();
            Cli::try_parse_from(words).unwrap_or_else(|error| panic!("{line}: {error}"));
            checked += 1;
        }
    }
    assert!(checked >= 4, "only {checked} commands found");
}
