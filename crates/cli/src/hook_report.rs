//! `sentinal hook report`: the background process a shell hook starts per event.
//!
//! It registers the shell (idempotent on the daemon) and sends one Command
//! start or finish over the private local socket, all within a 50 ms deadline.
//! Every failure is silent: the caller discards output and never waits.

use protocol::{
    local::{local_socket_path, DEFAULT_DATA_DIR},
    Envelope, LocalRequest, LocalResponse, ProtocolError, SessionId, Timestamp,
    MAX_COMMAND_TEXT_BYTES, MAX_LOCAL_FRAME_BYTES,
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

/// Whole-report transport deadline.
pub const REPORT_DEADLINE: Duration = Duration::from_millis(50);
/// Environment variable carrying Command text (never argv, which `ps` shows).
pub const TEXT_ENV: &str = "SENTINAL_HOOK_TEXT";

/// Which shell event is being reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ReportKind {
    /// The shell started; register it.
    Register,
    /// A Command is starting.
    Start,
    /// A Command finished.
    Finish,
}

/// One fully resolved report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookReport {
    /// Event kind.
    pub kind: ReportKind,
    /// Shell PID (`$$`).
    pub pid: u32,
    /// Shell name or path.
    pub shell: String,
    /// Per-shell Command sequence (start/finish).
    pub sequence: u64,
    /// Command exit status (finish).
    pub exit_code: i32,
    /// Shell-observed event time.
    pub at: Timestamp,
    /// Exact Command text (start).
    pub text: String,
    /// Working directory.
    pub cwd: String,
    /// Optional Session title from `SENTINAL_TITLE`.
    pub title: Option<String>,
}

/// Why a report was not delivered (never shown to the user).
#[derive(Debug, thiserror::Error)]
pub enum ReportError {
    /// Reporting is suppressed in this environment.
    #[error("reporting is suppressed")]
    Suppressed,
    /// The deadline passed.
    #[error("local report timed out")]
    Timeout,
    /// The socket could not be used.
    #[error("local socket failed: {0}")]
    Io(#[from] std::io::Error),
    /// A frame could not be encoded or decoded.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// The reply was oversized or not what the request expects.
    #[error("unexpected local reply: {0}")]
    Reply(String),
}

/// Parse `--at`: Unix seconds with a `.` or `,` fraction (`$EPOCHREALTIME`).
pub fn parse_epoch(value: &str) -> Option<Timestamp> {
    let (seconds, fraction) = value.split_once(['.', ',']).unwrap_or((value, ""));
    let seconds: i64 = seconds.parse().ok()?;
    let millis = fraction
        .chars()
        .chain(std::iter::repeat('0'))
        .take(3)
        .collect::<String>()
        .parse::<i64>()
        .ok()?;
    Some(Timestamp(seconds.checked_mul(1_000)?.checked_add(millis)?))
}

/// Current wall-clock time.
pub fn now() -> Timestamp {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    Timestamp(i64::try_from(millis).unwrap_or(i64::MAX))
}

/// Truncate Command text to the protocol limit at a character boundary.
pub fn bounded_text(mut text: String) -> String {
    if text.len() > MAX_COMMAND_TEXT_BYTES {
        let mut end = MAX_COMMAND_TEXT_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}

/// Socket path from `SENTINAL_SOCKET`, else the installed daemon.toml's
/// `local_socket`, else the daemon's default location.
pub fn socket_path(
    explicit: Option<PathBuf>,
    home: Option<&Path>,
    xdg_runtime_dir: Option<&Path>,
) -> Option<PathBuf> {
    explicit.filter(|path| path.is_absolute()).or_else(|| {
        home.map(|home| {
            configured_socket(&home.join(".config/terminal-agent/daemon.toml")).unwrap_or_else(
                || {
                    local_socket_path(
                        &home.join(DEFAULT_DATA_DIR),
                        xdg_runtime_dir,
                        cfg!(target_os = "macos"),
                    )
                },
            )
        })
    })
}

/// The absolute `local_socket` an installer pinned in `config`, if any.
fn configured_socket(config: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(config).ok()?;
    let table = text.parse::<toml::Table>().ok()?;
    let path = PathBuf::from(table.get("local_socket")?.as_str()?);
    path.is_absolute().then_some(path)
}

/// Whether `SENTINAL_SESSION_ID` or `SENTINAL_DISABLE=1` suppresses reporting.
pub fn suppressed(session_id: Option<&str>, disable: Option<&str>) -> bool {
    session_id.is_some_and(|id| !id.is_empty()) || disable == Some("1")
}

/// Deliver `report` within `deadline`.
pub async fn send(
    socket: &Path,
    report: &HookReport,
    deadline: Duration,
) -> Result<(), ReportError> {
    tokio::time::timeout(deadline, deliver(socket, report))
        .await
        .map_err(|_| ReportError::Timeout)?
}

async fn deliver(socket: &Path, report: &HookReport) -> Result<(), ReportError> {
    let session_id = match exchange(
        socket,
        LocalRequest::RegisterShell {
            pid: report.pid,
            shell: report.shell.clone(),
            cwd: report.cwd.clone(),
            title: report.title.clone(),
        },
    )
    .await?
    {
        LocalResponse::Session { session } => session.id,
        other => return Err(ReportError::Reply(kind_of(&other))),
    };
    let request = match report.kind {
        ReportKind::Register => return Ok(()),
        ReportKind::Start => started(report, session_id),
        ReportKind::Finish => LocalRequest::ShellCommandFinished {
            session_id,
            pid: report.pid,
            sequence: report.sequence,
            exit_code: report.exit_code,
            finished_at: report.at,
        },
    };
    request.validate()?;
    match exchange(socket, request).await? {
        LocalResponse::Error { code, .. } => Err(ReportError::Reply(format!("{code:?}"))),
        _ => Ok(()),
    }
}

fn started(report: &HookReport, session_id: SessionId) -> LocalRequest {
    LocalRequest::ShellCommandStarted {
        session_id,
        pid: report.pid,
        sequence: report.sequence,
        text: report.text.clone(),
        cwd: report.cwd.clone(),
        started_at: report.at,
    }
}

/// Send one framed request and read one framed reply.
pub async fn exchange(socket: &Path, request: LocalRequest) -> Result<LocalResponse, ReportError> {
    let mut stream = UnixStream::connect(socket).await?;
    let body = Envelope::new(request).encode()?;
    let length =
        u32::try_from(body.len()).map_err(|_| ReportError::Reply("request too large".into()))?;
    stream.write_all(&length.to_be_bytes()).await?;
    stream.write_all(body.as_bytes()).await?;
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_LOCAL_FRAME_BYTES {
        return Err(ReportError::Reply(format!("reply length {length}")));
    }
    let mut reply = vec![0_u8; length];
    stream.read_exact(&mut reply).await?;
    let text =
        std::str::from_utf8(&reply).map_err(|error| ReportError::Reply(error.to_string()))?;
    Ok(Envelope::<LocalResponse>::decode(text)?.payload)
}

fn kind_of(response: &LocalResponse) -> String {
    match response {
        LocalResponse::Session { .. } => "SESSION".into(),
        LocalResponse::Command { .. } => "COMMAND".into(),
        LocalResponse::Identity { .. } => "IDENTITY".into(),
        LocalResponse::Connection { .. } => "CONNECTION".into(),
        LocalResponse::ApprovalRequested { .. } => "APPROVAL_REQUESTED".into(),
        LocalResponse::ApprovalState { .. } => "APPROVAL_STATE".into(),
        LocalResponse::Error { code, .. } => format!("ERROR {code:?}"),
    }
}
