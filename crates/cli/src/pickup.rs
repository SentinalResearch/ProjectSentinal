//! Picking up a Session with `sentinal watch` (T4.1): which Sessions can be
//! picked up, in what order, how a name finds one, and how the list reads.
//! Pure: no I/O, no clock. `docs/WATCH_PICKUP_DESIGN.md`.

use protocol::{CaptureLevel, ProjectedSession, Session, SessionState, Timestamp};
use std::fmt::Write as _;

/// Most rows the picker shows.
pub const MAX_ROWS: usize = 20;

/// A Session that can be picked up, from this Machine or through the control.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Candidate {
    /// The Machine's ID; `None` for this Machine.
    pub machine_id: Option<String>,
    /// The Machine's name as shown.
    pub machine: String,
    /// The Session's number on its Machine.
    pub id: u64,
    /// What the Session is called: its title, else its shell.
    pub name: String,
    /// The coding agent running in it, lower case (`claude`).
    pub agent: Option<String>,
    /// The last folder of its directory (this Machine only: the control's
    /// read model leaves the directory out).
    pub folder: Option<String>,
    /// Its state.
    pub state: SessionState,
    /// When it started.
    pub started_at: Timestamp,
    /// When its state last changed.
    pub changed_at: Timestamp,
}

fn agent_name(agent: Option<&protocol::coding_agent::CodingAgentSummary>) -> Option<String> {
    agent.map(|summary| format!("{:?}", summary.agent).to_lowercase())
}

/// A FULL, live Session: one there is a terminal to pick up.
fn can_pick_up(state: SessionState, capture: CaptureLevel) -> bool {
    capture == CaptureLevel::Full && !state.is_finished() && state != SessionState::Disconnected
}

impl Candidate {
    /// A Session on this Machine, if it can be picked up.
    pub fn local(session: &Session, machine: &str) -> Option<Self> {
        can_pick_up(session.state, session.capture).then(|| Candidate {
            machine_id: None,
            machine: machine.to_string(),
            id: session.id.0,
            name: session
                .title
                .clone()
                .unwrap_or_else(|| session.shell.clone()),
            agent: agent_name(session.coding_agent.as_deref()),
            folder: std::path::Path::new(&session.cwd)
                .file_name()
                .map(|name| name.to_string_lossy().into_owned()),
            state: session.state,
            started_at: session.created_at,
            changed_at: session.updated_at,
        })
    }

    /// A Session on another Machine, from the control's read model.
    pub fn remote(session: &ProjectedSession, machine_id: &str, machine: &str) -> Option<Self> {
        can_pick_up(session.state, session.capture).then(|| Candidate {
            machine_id: Some(machine_id.to_string()),
            machine: machine.to_string(),
            id: session.id.0,
            name: session
                .title
                .clone()
                .unwrap_or_else(|| format!("Session {}", session.id.0)),
            agent: agent_name(session.coding_agent.as_ref()),
            folder: None,
            state: session.state,
            started_at: session.created_at,
            changed_at: session.updated_at,
        })
    }

    /// `ID` on this Machine, `MACHINE:ID` elsewhere.
    pub fn label(&self) -> String {
        match &self.machine_id {
            None => self.id.to_string(),
            Some(_) => format!("{}:{}", self.machine, self.id),
        }
    }
}

/// Waiting for the owner first, then working, then at a prompt.
fn rank(state: SessionState) -> u8 {
    match state {
        SessionState::Waiting => 0,
        SessionState::Stalled => 1,
        SessionState::Running => 2,
        _ => 3,
    }
}

/// Live Sessions in the picker's order: waiting first, then running, then
/// idle; the most recently changed first within each.
pub fn order(mut candidates: Vec<Candidate>) -> Vec<Candidate> {
    candidates.sort_by(|a, b| {
        rank(a.state)
            .cmp(&rank(b.state))
            .then(b.changed_at.0.cmp(&a.changed_at.0))
            .then(a.machine.cmp(&b.machine))
            .then(a.id.cmp(&b.id))
    });
    candidates
}

/// The most recently changed live Session (`--last`).
pub fn last(candidates: &[Candidate]) -> Option<&Candidate> {
    candidates
        .iter()
        .max_by_key(|candidate| candidate.changed_at.0)
}

/// What a `NAME|ID` found.
#[derive(Debug, Clone, PartialEq)]
pub enum Found<'a> {
    /// Exactly one Session.
    One(&'a Candidate),
    /// Several: the owner must say which.
    Many(Vec<&'a Candidate>),
    /// None.
    Nothing,
}

/// Find `query`, first rule with any match winning: `ID` or `MACHINE:ID`;
/// the title exactly; the agent's name or the folder exactly; a title, or a
/// word in it, that starts with it. Case does not matter.
pub fn find<'a>(candidates: &'a [Candidate], query: &str) -> Found<'a> {
    let query = query.trim();
    let wanted = query.to_lowercase();
    let by_id: Vec<&Candidate> = match query.rsplit_once(':') {
        Some((machine, id)) => id.parse::<u64>().map_or_else(
            |_| Vec::new(),
            |id| {
                candidates
                    .iter()
                    .filter(|c| {
                        c.id == id
                            && (c.machine.eq_ignore_ascii_case(machine)
                                || c.machine_id.as_deref() == Some(machine))
                    })
                    .collect()
            },
        ),
        None => query.parse::<u64>().map_or_else(
            |_| Vec::new(),
            |id| {
                // A bare number means this Machine first, then anywhere.
                let here: Vec<&Candidate> = candidates
                    .iter()
                    .filter(|c| c.id == id && c.machine_id.is_none())
                    .collect();
                if here.is_empty() {
                    candidates.iter().filter(|c| c.id == id).collect()
                } else {
                    here
                }
            },
        ),
    };
    let rules: [&dyn Fn(&Candidate) -> bool; 3] = [
        &|c| c.name.to_lowercase() == wanted,
        &|c| {
            c.agent.as_deref() == Some(wanted.as_str())
                || c.folder.as_deref().map(str::to_lowercase).as_deref() == Some(wanted.as_str())
        },
        &|c| {
            let name = c.name.to_lowercase();
            !wanted.is_empty()
                && (name.starts_with(&wanted)
                    || name
                        .split(|ch: char| !ch.is_alphanumeric())
                        .any(|word| word.starts_with(&wanted)))
        },
    ];
    let mut found = by_id;
    for rule in rules {
        if !found.is_empty() {
            break;
        }
        found = candidates.iter().filter(|c| rule(c)).collect();
    }
    match found.len() {
        0 => Found::Nothing,
        1 => Found::One(found[0]),
        _ => Found::Many(found),
    }
}

/// What the owner typed at the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// The row with this index, from 0.
    Pick(usize),
    /// Start a new shell Session.
    New,
    /// Not an answer; ask again.
    Again,
}

/// Read one answer: a row number, `n` for new, Enter for the first row.
pub fn choice(answer: &str, rows: usize) -> Choice {
    match answer.trim() {
        "" if rows > 0 => Choice::Pick(0),
        "n" | "N" | "new" => Choice::New,
        number => match number.parse::<usize>() {
            Ok(row) if (1..=rows).contains(&row) => Choice::Pick(row - 1),
            _ => Choice::Again,
        },
    }
}

/// `2h 10m`, `41m`, `3d`.
pub fn running_for(started: Timestamp, now: Timestamp) -> String {
    let minutes = now.0.saturating_sub(started.0).max(0) / 60_000;
    match minutes {
        0 => "<1m".into(),
        1..=59 => format!("{minutes}m"),
        60..=1_439 => match minutes % 60 {
            0 => format!("{}h", minutes / 60),
            rest => format!("{}h {rest}m", minutes / 60),
        },
        _ => format!("{}d", minutes / 1_440),
    }
}

fn state_word(state: SessionState) -> &'static str {
    match state {
        SessionState::Waiting => "WAITING",
        SessionState::Stalled => "STALLED",
        SessionState::Running => "RUNNING",
        SessionState::Idle => "IDLE",
        _ => "LIVE",
    }
}

/// The picker's table, numbered from 1, at most [`MAX_ROWS`] rows.
pub fn render(candidates: &[Candidate], now: Timestamp) -> String {
    let shown = &candidates[..candidates.len().min(MAX_ROWS)];
    let machine_width = shown
        .iter()
        .map(|c| c.machine.chars().count())
        .max()
        .unwrap_or(0)
        .max("MACHINE".len());
    let name_width = shown
        .iter()
        .map(|c| display_name(c).chars().count())
        .max()
        .unwrap_or(0)
        .clamp("SESSION".len(), 40);
    let mut text = String::new();
    let _ = writeln!(
        text,
        "  #  {:<machine_width$}  {:<name_width$}  {:<8}  RUNNING",
        "MACHINE", "SESSION", "STATE"
    );
    for (index, candidate) in shown.iter().enumerate() {
        let name: String = display_name(candidate).chars().take(name_width).collect();
        let _ = writeln!(
            text,
            "{:>3}  {:<machine_width$}  {:<name_width$}  {:<8}  {}",
            index + 1,
            candidate.machine,
            name,
            state_word(candidate.state),
            running_for(candidate.started_at, now),
        );
    }
    if candidates.len() > shown.len() {
        let _ = writeln!(
            text,
            "  and {} more: sentinal ls --all",
            candidates.len() - shown.len()
        );
    }
    text
}

/// `claude · api` when the agent and folder add to the name.
fn display_name(candidate: &Candidate) -> String {
    let mut parts = vec![candidate.name.clone()];
    if let Some(agent) = &candidate.agent {
        if !candidate.name.to_lowercase().contains(agent.as_str()) {
            parts.insert(0, agent.clone());
        }
    }
    if let Some(folder) = &candidate.folder {
        if !parts.iter().any(|part| part.contains(folder.as_str())) {
            parts.push(folder.clone());
        }
    }
    parts.join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(
        machine: Option<&str>,
        id: u64,
        name: &str,
        state: SessionState,
        changed: i64,
    ) -> Candidate {
        Candidate {
            machine_id: machine.map(|m| format!("{m}-id")),
            machine: machine.unwrap_or("laptop").into(),
            id,
            name: name.into(),
            agent: None,
            folder: None,
            state,
            started_at: Timestamp(0),
            changed_at: Timestamp(changed),
        }
    }

    fn sample() -> Vec<Candidate> {
        let mut claude = candidate(Some("gpu-box"), 4, "api", SessionState::Waiting, 10);
        claude.agent = Some("claude".into());
        let mut npm = candidate(None, 3, "npm dev", SessionState::Idle, 50);
        npm.folder = Some("web".into());
        vec![
            npm,
            candidate(None, 7, "layout-tests queue", SessionState::Running, 40),
            claude,
            candidate(None, 4, "zsh", SessionState::Running, 60),
        ]
    }

    #[test]
    fn waiting_comes_first_then_running_newest_first_then_idle() {
        let names: Vec<String> = order(sample()).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["api", "zsh", "layout-tests queue", "npm dev"]);
    }

    #[test]
    fn last_is_the_most_recently_changed() {
        assert_eq!(last(&sample()).map(|c| c.name.as_str()), Some("zsh"));
        assert_eq!(last(&[]), None);
    }

    #[test]
    fn a_number_means_this_machine_first_then_anywhere() {
        let all = sample();
        assert!(matches!(find(&all, "4"), Found::One(c) if c.machine_id.is_none()));
        assert!(matches!(find(&all, "gpu-box:4"), Found::One(c) if c.name == "api"));
        assert!(matches!(find(&all, "gpu-box-id:4"), Found::One(c) if c.name == "api"));
        assert_eq!(find(&all, "99"), Found::Nothing);
        let elsewhere = vec![candidate(
            Some("gpu-box"),
            9,
            "train",
            SessionState::Running,
            0,
        )];
        assert!(matches!(find(&elsewhere, "9"), Found::One(_)));
    }

    #[test]
    fn names_match_title_then_agent_or_folder_then_a_title_start() {
        let all = sample();
        assert!(matches!(find(&all, "NPM DEV"), Found::One(c) if c.id == 3));
        assert!(matches!(find(&all, "claude"), Found::One(c) if c.name == "api"));
        assert!(matches!(find(&all, "web"), Found::One(c) if c.name == "npm dev"));
        assert!(matches!(find(&all, "layout"), Found::One(c) if c.id == 7));
        // Any word of the title, as people remember it.
        assert!(matches!(find(&all, "queue"), Found::One(c) if c.id == 7));
        assert!(matches!(find(&all, "tests"), Found::One(c) if c.id == 7));
        assert_eq!(find(&all, "nothing-like-it"), Found::Nothing);
    }

    #[test]
    fn an_ambiguous_name_lists_the_matches() {
        let mut all = sample();
        let mut second = candidate(None, 8, "api tests", SessionState::Running, 0);
        second.agent = Some("claude".into());
        all.push(second);
        match find(&all, "claude") {
            Found::Many(matches) => assert_eq!(matches.len(), 2),
            other => panic!("{other:?}"),
        }
        // An exact title beats a title start.
        assert!(matches!(find(&all, "api"), Found::One(c) if c.id == 4));
    }

    #[test]
    fn the_picker_reads_a_number_n_or_enter() {
        assert_eq!(choice("", 3), Choice::Pick(0));
        assert_eq!(choice(" 2 ", 3), Choice::Pick(1));
        assert_eq!(choice("n", 3), Choice::New);
        assert_eq!(choice("4", 3), Choice::Again);
        assert_eq!(choice("0", 3), Choice::Again);
        assert_eq!(choice("x", 3), Choice::Again);
    }

    #[test]
    fn running_times_read_short() {
        let now = Timestamp(3 * 86_400_000);
        assert_eq!(running_for(Timestamp(now.0 - 30_000), now), "<1m");
        assert_eq!(running_for(Timestamp(now.0 - 41 * 60_000), now), "41m");
        assert_eq!(running_for(Timestamp(now.0 - 130 * 60_000), now), "2h 10m");
        assert_eq!(running_for(Timestamp(now.0 - 120 * 60_000), now), "2h");
        assert_eq!(running_for(Timestamp(0), now), "3d");
    }

    #[test]
    fn the_table_numbers_rows_and_says_what_is_left_out() {
        let ordered = order(sample());
        let table = render(&ordered, Timestamp(60_000 * 5));
        let lines: Vec<&str> = table.lines().collect();
        assert!(lines[0].contains("MACHINE") && lines[0].contains("SESSION"));
        assert!(lines[1].starts_with("  1  gpu-box") && lines[1].contains("claude · api"));
        assert!(lines[1].contains("WAITING") && lines[1].ends_with("5m"));
        assert!(lines[4].contains("npm dev · web"));

        let many: Vec<Candidate> = (0..25)
            .map(|id| candidate(None, id, "job", SessionState::Running, 0))
            .collect();
        let table = render(&many, Timestamp(0));
        assert_eq!(table.lines().count(), 1 + MAX_ROWS + 1);
        assert!(table.ends_with("and 5 more: sentinal ls --all\n"));
    }

    #[test]
    fn finished_reduced_and_unreachable_sessions_cannot_be_picked_up() {
        assert!(can_pick_up(SessionState::Idle, CaptureLevel::Full));
        assert!(!can_pick_up(SessionState::Completed, CaptureLevel::Full));
        assert!(!can_pick_up(SessionState::Disconnected, CaptureLevel::Full));
        assert!(!can_pick_up(
            SessionState::Running,
            CaptureLevel::ProcessLog
        ));
    }
}
