//! Milestones 6.4 and 6.5: the skill file agents install, `sentinal
//! agent-state`, and `sentinal agent-skill --install`.

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

// ---- Milestone 6.5 ----

use cli::agent_state_command::{build_request, Report, StatusArg};
use cli::skill_install::{self, Outcome};
use protocol::{AgentStatus, LocalRequest, SessionId};

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
    let mut all = vec!["sentinal", "agent-state"];
    all.extend_from_slice(args);
    Cli::try_parse_from(all)
}

#[test]
fn step_of_and_eta_parse_into_one_report() {
    let cli = parse(&[
        "--status",
        "working",
        "--task",
        "migrate tests",
        "--step",
        "3",
        "--of",
        "8",
        "--eta",
        "12m",
    ])
    .unwrap();
    let cli::args::Command::AgentState { report, .. } = cli.command else {
        panic!("not agent-state");
    };
    assert_eq!(
        build_request(Some("7"), report).unwrap(),
        Some(LocalRequest::AgentState {
            session_id: SessionId(7),
            status: AgentStatus::Working,
            detail: None,
            task: Some("migrate tests".into()),
            step: Some(3),
            steps: Some(8),
            eta_secs: Some(720),
            caller_pid: None,
        })
    );
}

#[test]
fn a_bad_eta_or_step_is_a_usage_error() {
    for args in [
        &["--status", "working", "--eta", "soon"][..],
        &["--status", "working", "--eta", "8d"],
        &["--status", "working", "--step", "-1"],
        &["--status", "working", "--of", "many"],
    ] {
        assert_eq!(parse(args).unwrap_err().exit_code(), 2, "{args:?}");
    }
}

#[test]
fn a_step_past_the_plan_or_a_long_task_is_refused_even_outside_a_session() {
    let report = |task: &str, step: u32, steps: u32| Report {
        status: StatusArg::Working,
        detail: None,
        task: Some(task.into()),
        step: Some(step),
        steps: Some(steps),
        eta_secs: None,
    };
    assert!(build_request(None, report("x", 9, 8)).is_err());
    assert!(build_request(None, report(&"t".repeat(81), 1, 8)).is_err());
    assert_eq!(build_request(None, report("x", 3, 8)), Ok(None));
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sentinal-skill-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn install_writes_only_after_the_owner_types_yes() {
    let dir = temp_dir("yes");
    let file = dir.join("AGENTS.md");
    std::fs::write(&file, "# Rules\n\nBe careful.\n").unwrap();
    let mut out = Vec::new();
    let mut asked = None;
    let outcome = skill_install::install(
        &dir,
        None,
        "SKILL\n",
        &mut |diff| {
            asked = Some(diff.to_string());
            Some(true)
        },
        &mut out,
    )
    .unwrap();
    assert_eq!(outcome, Outcome::Written(file.clone()));
    let diff = asked.unwrap();
    assert!(diff.contains("+<!-- sentinal:agent-skill -->"), "{diff}");
    assert!(diff.contains("+SKILL"), "{diff}");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.starts_with("# Rules\n\nBe careful.\n"));
    assert!(text.contains("<!-- sentinal:agent-skill -->\nSKILL\n<!-- /sentinal:agent-skill -->"));

    // Running again with a newer skill replaces only the marked section.
    skill_install::install(&dir, None, "SKILL v2\n", &mut |_| Some(true), &mut out).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("SKILL v2") && !text.contains("SKILL\n<!--"));
    assert_eq!(text.matches("<!-- sentinal:agent-skill -->").count(), 1);
    assert!(text.starts_with("# Rules"));
    let again = skill_install::install(
        &dir,
        None,
        "SKILL v2\n",
        &mut |_| panic!("nothing to confirm"),
        &mut out,
    )
    .unwrap();
    assert_eq!(again, Outcome::UpToDate(file));
}

#[test]
fn install_without_a_terminal_or_with_a_no_writes_nothing() {
    let dir = temp_dir("no");
    let file = dir.join("CLAUDE.md");
    std::fs::write(&file, "mine\n").unwrap();
    let mut out = Vec::new();
    // No terminal (an agent's tool call): the diff is shown, nothing written.
    let outcome = skill_install::install(&dir, None, "SKILL\n", &mut |_| None, &mut out).unwrap();
    assert_eq!(outcome, Outcome::NeedsOwner(file.clone()));
    assert!(String::from_utf8_lossy(&out).contains("+SKILL"));
    let outcome =
        skill_install::install(&dir, None, "SKILL\n", &mut |_| Some(false), &mut out).unwrap();
    assert_eq!(outcome, Outcome::Declined(file.clone()));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "mine\n");
}

#[test]
fn install_picks_agents_md_then_claude_md_then_a_new_agents_md() {
    let dir = temp_dir("pick");
    assert_eq!(skill_install::target(&dir, None), dir.join("AGENTS.md"));
    std::fs::write(dir.join("CLAUDE.md"), "").unwrap();
    assert_eq!(skill_install::target(&dir, None), dir.join("CLAUDE.md"));
    std::fs::write(dir.join("AGENTS.md"), "").unwrap();
    assert_eq!(skill_install::target(&dir, None), dir.join("AGENTS.md"));
    let chosen = dir.join("docs/AI.md");
    assert_eq!(skill_install::target(&dir, Some(&chosen)), chosen);
}

#[test]
fn there_is_no_yes_flag() {
    assert!(Cli::try_parse_from(["sentinal", "agent-skill", "--install", "--yes"]).is_err());
    assert!(Cli::try_parse_from(["sentinal", "agent-skill", "--install"]).is_ok());
}
