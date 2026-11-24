//! `sentinal watch` without a Command: pick up a live Session here or on
//! another Machine (T4). The rules are in [`crate::pickup`];
//! `docs/WATCH_PICKUP_DESIGN.md` has the design.

use crate::client::{AttachEnd, ClientError, DaemonClient};
use crate::login;
use crate::pickup::{self, Candidate, Choice, Found};
use protocol::{ClientRequest, DaemonMessage, SessionId, Timestamp};
use std::io::{BufRead, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

/// What the owner asked to pick up.
#[derive(Debug, Clone, Default)]
pub struct Pick {
    /// `NAME|ID`.
    pub target: Option<String>,
    /// `--last`.
    pub last: bool,
    /// `--machine NAME`.
    pub machine: Option<String>,
    /// `--json` (without a terminal).
    pub json: bool,
}

/// Tries at the picker before giving up.
const PICKER_TRIES: usize = 3;

/// Which Machines the list covers, for its last line.
enum Reach {
    /// The owner is signed in to the control: every Machine.
    Everywhere,
    /// Only this Machine; the reason, for the owner.
    HereOnly(&'static str),
}

/// Pick up a Session. `Ok(None)`: nothing to pick up on a terminal, so the
/// caller starts a new shell Session, as a bare `watch` always did.
pub async fn run(
    pick: Pick,
    urls: &[String],
    token_file: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Option<ExitCode>, ClientError> {
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let inside = std::env::var(crate::watch::SESSION_ENV)
        .ok()
        .filter(|id| !id.is_empty());
    if let Some(session) = inside {
        if pick.target.is_none() && !pick.last {
            // `watch` itself says this terminal is already watched.
            return Ok(None);
        }
        let _ = writeln!(
            err,
            "sentinal: this terminal is already inside Session {session}; picking up another here would nest terminals. Run it from a terminal outside Sentinal."
        );
        return Ok(Some(ExitCode::FAILURE));
    }

    let now = crate::hook_report::now();
    let (candidates, owner, reach) = gather(urls, token_file, now).await?;
    let candidates = pickup::order(match &pick.machine {
        Some(machine) => candidates
            .into_iter()
            .filter(|c| {
                c.machine.eq_ignore_ascii_case(machine)
                    || c.machine_id.as_deref() == Some(machine.as_str())
            })
            .collect(),
        None => candidates,
    });

    let chosen = if let Some(target) = &pick.target {
        match pickup::find(&candidates, target) {
            Found::One(candidate) => candidate.clone(),
            Found::Many(matches) => {
                let _ = writeln!(
                    err,
                    "sentinal: {} Sessions match `{target}`; pick one by its ID:",
                    matches.len()
                );
                let matches: Vec<Candidate> = matches.into_iter().cloned().collect();
                let _ = write!(err, "{}", with_ids(&matches, now));
                return Ok(Some(ExitCode::FAILURE));
            }
            Found::Nothing => {
                let _ = writeln!(err, "sentinal: no live Session matches `{target}`.");
                list(&candidates, &reach, now, false, err);
                return Ok(Some(ExitCode::FAILURE));
            }
        }
    } else if pick.last {
        match pickup::last(&candidates) {
            Some(candidate) => candidate.clone(),
            None => {
                let _ = writeln!(err, "sentinal: nothing is running to pick up.");
                return Ok(Some(ExitCode::FAILURE));
            }
        }
    } else if !interactive {
        // A script or an agent: never prompt; say what there is.
        if pick.json {
            let _ = writeln!(
                out,
                "{}",
                serde_json::to_string(&candidates).unwrap_or_default()
            );
        } else {
            list(&candidates, &reach, now, true, out);
        }
        let _ = writeln!(
            err,
            "sentinal: no terminal to choose on; give an ID, a name, --last, --new, or -- COMMAND"
        );
        return Ok(Some(ExitCode::from(2)));
    } else {
        match candidates.len() {
            0 => {
                let _ = writeln!(
                    err,
                    "sentinal: nothing is running; starting a new shell Session."
                );
                return Ok(None);
            }
            1 => {
                let _ = writeln!(err, "sentinal: picking up {}.", describe(&candidates[0]));
                candidates[0].clone()
            }
            _ => match ask(&candidates, &reach, now, err)? {
                Some(candidate) => candidate,
                None => return Ok(None),
            },
        }
    };

    attach(&chosen, urls, token_file, owner.as_ref(), err)
        .await
        .map(Some)
}

/// This Machine's live Sessions, and every other Machine's when signed in.
async fn gather(
    urls: &[String],
    token_file: &Path,
    now: Timestamp,
) -> Result<(Vec<Candidate>, Option<login::OwnerSession>, Reach), ClientError> {
    let mut client = DaemonClient::connect(urls, token_file).await?;
    let (here_id, here_name) = match client.request(ClientRequest::GetMachine).await? {
        DaemonMessage::Machine { machine } => (machine.id.0, machine.name),
        _ => (String::new(), "this Machine".to_string()),
    };
    let mut candidates: Vec<Candidate> = match client.request(ClientRequest::ListSessions).await? {
        DaemonMessage::Sessions { sessions } => sessions
            .iter()
            .filter_map(|session| Candidate::local(session, &here_name))
            .collect(),
        DaemonMessage::Error { message } => return Err(ClientError::Daemon(message)),
        _ => Vec::new(),
    };
    let owner = login::load(&login::session_path(token_file), now);
    let reach = match &owner {
        None => Reach::HereOnly("Only this Machine. `sentinal login` to see your other Machines."),
        Some(owner) => match login::snapshot(owner).await {
            Ok(snapshot) => {
                for machine in snapshot
                    .machines
                    .iter()
                    .filter(|m| m.machine_id.0 != here_id)
                {
                    candidates.extend(machine.sessions.iter().filter_map(|session| {
                        Candidate::remote(session, &machine.machine_id.0, &machine.display_name)
                    }));
                }
                Reach::Everywhere
            }
            Err(login::RemoteError::SignedOut) => Reach::HereOnly(
                "Only this Machine: the sign-in to the control ended. `sentinal login` again.",
            ),
            Err(_) => Reach::HereOnly("Only this Machine: the control could not be reached."),
        },
    };
    Ok((candidates, owner, reach))
}

fn describe(candidate: &Candidate) -> String {
    format!(
        "{} on {} ({})",
        candidate.name,
        candidate.machine,
        candidate.label()
    )
}

fn with_ids(candidates: &[Candidate], now: Timestamp) -> String {
    let table = pickup::render(candidates, now);
    let lines: Vec<&str> = table.lines().collect();
    let width = lines
        .iter()
        .take(candidates.len().min(pickup::MAX_ROWS) + 1)
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let mut text = String::new();
    for (index, line) in lines.iter().enumerate() {
        let id = match index {
            0 => "ID".to_string(),
            row => match candidates.get(row - 1) {
                Some(candidate) if row <= pickup::MAX_ROWS => candidate.label(),
                _ => {
                    text.push_str(line);
                    text.push('\n');
                    continue;
                }
            },
        };
        text.push_str(&format!("{line:<width$}  {id}\n"));
    }
    text
}

fn list(candidates: &[Candidate], reach: &Reach, now: Timestamp, ids: bool, out: &mut dyn Write) {
    if candidates.is_empty() {
        let _ = writeln!(out, "Nothing is running.");
    } else if ids {
        let _ = write!(out, "{}", with_ids(candidates, now));
    } else {
        let _ = write!(out, "{}", pickup::render(candidates, now));
    }
    if let Reach::HereOnly(note) = reach {
        let _ = writeln!(out, "{note}");
    }
}

/// The picker: the list, then a number from the terminal.
fn ask(
    candidates: &[Candidate],
    reach: &Reach,
    now: Timestamp,
    err: &mut dyn Write,
) -> Result<Option<Candidate>, ClientError> {
    list(candidates, reach, now, false, err);
    let rows = candidates.len().min(pickup::MAX_ROWS);
    let tty = std::fs::File::open("/dev/tty").map_err(|source| ClientError::Io {
        operation: "open the terminal",
        source,
    })?;
    let mut lines = std::io::BufReader::new(tty).lines();
    for _ in 0..PICKER_TRIES {
        let _ = write!(
            err,
            "Pick a Session (1–{rows}), n for a new one, Enter for 1: "
        );
        let _ = err.flush();
        let Some(Ok(answer)) = lines.next() else {
            return Ok(None);
        };
        match pickup::choice(&answer, rows) {
            Choice::Pick(row) => return Ok(Some(candidates[row].clone())),
            Choice::New => return Ok(None),
            Choice::Again => {}
        }
    }
    let _ = writeln!(err, "sentinal: no Session picked.");
    Err(ClientError::Daemon("no Session picked".into()))
}

/// Attach to `candidate`, here or through the control.
async fn attach(
    candidate: &Candidate,
    urls: &[String],
    token_file: &Path,
    owner: Option<&login::OwnerSession>,
    err: &mut dyn Write,
) -> Result<ExitCode, ClientError> {
    let client = match (&candidate.machine_id, owner) {
        (None, _) => DaemonClient::connect(urls, token_file).await?,
        (Some(machine_id), Some(owner)) => {
            DaemonClient::connect_with_token(
                &login::terminal_url(&owner.control_url, machine_id),
                &owner.token,
            )
            .await?
        }
        (Some(_), None) => {
            let _ = writeln!(
                err,
                "sentinal: `sentinal login` first to reach other Machines."
            );
            return Ok(ExitCode::FAILURE);
        }
    };
    let label = candidate.label();
    match client.pick_up(SessionId(candidate.id)).await? {
        AttachEnd::Detached => {
            let _ = writeln!(
                err,
                "sentinal: detached from {}; it keeps running. Pick it up again with `sentinal watch {label}`",
                describe(candidate)
            );
        }
        AttachEnd::Ended { exit_code } => {
            let _ = match exit_code {
                Some(code) => writeln!(
                    err,
                    "sentinal: {} ended (exit {code}).",
                    describe(candidate)
                ),
                None => writeln!(err, "sentinal: {} ended.", describe(candidate)),
            };
        }
    }
    Ok(ExitCode::SUCCESS)
}
