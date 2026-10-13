//! `sentinal adopt PID | --pick`: observe an existing process read-only.
//!
//! Adoption goes over the private local socket (never the remote API). The
//! CLI only lists processes (`ps`) and sends one request; it never signals or
//! touches the target.

use crate::hook_report::{exchange, socket_path};
use protocol::{local::SOCKET_ENV, CaptureLevel, LocalRequest, LocalResponse, Session};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

/// Interactive request deadline for adoption.
pub const ADOPT_DEADLINE: Duration = Duration::from_secs(2);
const MAX_CANDIDATES: usize = 40;

/// One `ps -A -o pid=,ppid=,uid=,etime=,args=` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PsRow {
    /// Process ID.
    pub pid: u32,
    /// Parent process ID.
    pub ppid: u32,
    /// Owning user ID.
    pub uid: u32,
    /// Elapsed time as `ps` prints it.
    pub elapsed: String,
    /// Command line.
    pub args: String,
}

/// Parse `ps` output, skipping lines that do not have all five columns.
pub fn parse_ps(output: &str) -> Vec<PsRow> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let pid = fields.next()?.parse().ok()?;
            let ppid = fields.next()?.parse().ok()?;
            let uid = fields.next()?.parse().ok()?;
            let elapsed = fields.next()?.to_string();
            let args = fields.collect::<Vec<_>>().join(" ");
            (!args.is_empty()).then_some(PsRow {
                pid,
                ppid,
                uid,
                elapsed,
                args,
            })
        })
        .collect()
}

/// Same-user processes worth offering, newest first. Excludes this process,
/// its parent shell, `ps` itself, and sentinald.
pub fn candidates(rows: &[PsRow], own_pid: u32) -> Vec<PsRow> {
    let Some(me) = rows.iter().find(|row| row.pid == own_pid) else {
        return Vec::new();
    };
    let mut found: Vec<PsRow> = rows
        .iter()
        .filter(|row| row.uid == me.uid)
        .filter(|row| row.pid != own_pid && row.pid != me.ppid && row.ppid != own_pid)
        .filter(|row| {
            let program = row.args.split_whitespace().next().unwrap_or_default();
            let name = program.rsplit('/').next().unwrap_or_default();
            !matches!(name, "sentinald" | "tad")
        })
        .cloned()
        .collect();
    found.sort_by_key(|row| {
        (
            elapsed_seconds(&row.elapsed).unwrap_or(u64::MAX),
            std::cmp::Reverse(row.pid),
        )
    });
    found.truncate(MAX_CANDIDATES);
    found
}

/// Parse the portable `ps etime` forms `[[dd-]hh:]mm:ss`.
fn elapsed_seconds(value: &str) -> Option<u64> {
    let (days, clock) = match value.split_once('-') {
        Some((days, clock)) => (days.parse::<u64>().ok()?, clock),
        None => (0, value),
    };
    let fields: Vec<u64> = clock
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let (hours, minutes, seconds) = match fields.as_slice() {
        [minutes, seconds] => (0, *minutes, *seconds),
        [hours, minutes, seconds] => (*hours, *minutes, *seconds),
        _ => return None,
    };
    (minutes < 60 && seconds < 60).then(|| {
        days.saturating_mul(86_400)
            .saturating_add(hours.saturating_mul(3_600))
            .saturating_add(minutes.saturating_mul(60))
            .saturating_add(seconds)
    })
}

/// What the user typed at the `--pick` prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    /// Adopt this PID.
    Adopt(u32),
    /// Nothing selected; adopt nothing.
    Cancel,
    /// Input that names no listed process.
    Invalid(String),
}

/// Interpret a list number or a listed PID; empty or `q` cancels.
pub fn choose(listed: &[PsRow], input: &str) -> Choice {
    let input = input.trim();
    if input.is_empty() || input.eq_ignore_ascii_case("q") {
        return Choice::Cancel;
    }
    let Ok(number) = input.parse::<u32>() else {
        return Choice::Invalid(format!("'{input}' is not a number"));
    };
    if let Some(row) = usize::try_from(number)
        .ok()
        .and_then(|index| index.checked_sub(1))
        .and_then(|index| listed.get(index))
    {
        return Choice::Adopt(row.pid);
    }
    if listed.iter().any(|row| row.pid == number) {
        return Choice::Adopt(number);
    }
    Choice::Invalid(format!(
        "{number} is neither a list number nor a listed PID"
    ))
}

/// Run `sentinal adopt`.
pub async fn run(
    pid: Option<u32>,
    pick: bool,
    log: Option<PathBuf>,
    title: Option<String>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> ExitCode {
    match adopt(pid, pick, log, title, input, out).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            let _ = writeln!(err, "sentinal: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn adopt(
    pid: Option<u32>,
    pick: bool,
    log: Option<PathBuf>,
    title: Option<String>,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<(), String> {
    let pid = match (pid, pick) {
        (Some(pid), _) => pid,
        (None, _) => match pick_interactively(input, out)? {
            Some(pid) => pid,
            None => return writeln!(out, "Nothing adopted.").map_err(|e| e.to_string()),
        },
    };
    if pid == std::process::id() {
        return Err(format!("pid {pid} is this sentinal command itself"));
    }
    let log_path = log.map(|path| checked_log(&path)).transpose()?;
    let socket = socket_path(
        std::env::var_os(SOCKET_ENV).map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from).as_deref(),
        std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .as_deref(),
    )
    .ok_or("HOME is not set; set SENTINAL_SOCKET")?;
    let request = LocalRequest::AdoptProcess {
        pid,
        title,
        log_path,
    };
    request.validate().map_err(|error| error.to_string())?;
    let reply = tokio::time::timeout(ADOPT_DEADLINE, exchange(&socket, request))
        .await
        .map_err(|_| {
            format!(
                "sentinald did not answer at {}; nothing was adopted",
                socket.display()
            )
        })?
        .map_err(|error| {
            format!(
                "cannot reach sentinald at {}: {error}; nothing was adopted",
                socket.display()
            )
        })?;
    match reply {
        LocalResponse::Session { session } => {
            writeln!(out, "{}", describe(pid, &session)).map_err(|e| e.to_string())
        }
        LocalResponse::Error { message, .. } => {
            Err(format!("sentinald refused to adopt pid {pid}: {message}"))
        }
        LocalResponse::Command { .. }
        | LocalResponse::Identity { .. }
        | LocalResponse::Connection { .. }
        | LocalResponse::ApprovalRequested { .. }
        | LocalResponse::ApprovalState { .. } => Err("sentinald sent an unexpected reply".into()),
    }
}

fn pick_interactively(input: &mut dyn BufRead, out: &mut dyn Write) -> Result<Option<u32>, String> {
    let output = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,uid=,etime=,args="])
        .output()
        .map_err(|error| format!("cannot list processes with ps: {error}"))?;
    let listed = candidates(
        &parse_ps(&String::from_utf8_lossy(&output.stdout)),
        std::process::id(),
    );
    if listed.is_empty() {
        return Err("no other processes of yours were found".into());
    }
    let write =
        |out: &mut dyn Write, text: String| writeln!(out, "{text}").map_err(|e| e.to_string());
    write(out, "Your processes (newest first):".into())?;
    for (index, row) in listed.iter().enumerate() {
        let args: String = row.args.chars().take(90).collect();
        write(
            out,
            format!(
                "{:>3}) {:>7}  {:>11}  {args}",
                index + 1,
                row.pid,
                row.elapsed
            ),
        )?;
    }
    write!(
        out,
        "Adopt which process? [number or PID, Enter to cancel]: "
    )
    .map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    let mut line = String::new();
    input.read_line(&mut line).map_err(|e| e.to_string())?;
    match choose(&listed, &line) {
        Choice::Adopt(pid) => Ok(Some(pid)),
        Choice::Cancel => Ok(None),
        Choice::Invalid(reason) => Err(format!("{reason}; nothing was adopted")),
    }
}

/// Resolve `--log` to an absolute, readable regular file before contacting the daemon.
fn checked_log(path: &Path) -> Result<String, String> {
    let absolute =
        std::path::absolute(path).map_err(|error| format!("--log {}: {error}", path.display()))?;
    let metadata = std::fs::metadata(&absolute)
        .map_err(|error| format!("--log {}: {error}", absolute.display()))?;
    if !metadata.is_file() {
        return Err(format!(
            "--log {} is not a regular file",
            absolute.display()
        ));
    }
    std::fs::File::open(&absolute)
        .map_err(|error| format!("--log {}: {error}", absolute.display()))?;
    Ok(absolute.to_string_lossy().into_owned())
}

/// Human summary of an adopted Session.
pub fn describe(pid: u32, session: &Session) -> String {
    let name = session
        .title
        .clone()
        .unwrap_or_else(|| format!("pid {pid}"));
    let observed = if session.capture == CaptureLevel::ProcessLog {
        "its log (read-only), Metrics, and lifecycle; no terminal input"
    } else {
        "Metrics and lifecycle, no output or input"
    };
    let mut text = format!(
        "Adopted pid {pid} ({name}) as Session {} [ADOPTED · {}].\nSentinal only observes {observed}. When it ends, its exit code will be unknown.",
        session.id.0, session.capture
    );
    if let Some(log) = &session.log {
        text.push_str(&if session.capture == CaptureLevel::ProcessLog {
            format!("\nWatching {} read-only.", log.path)
        } else {
            format!(
                "\nLog {} is not being read; Sentinal reports Metrics only.",
                log.path
            )
        });
    }
    text
}
