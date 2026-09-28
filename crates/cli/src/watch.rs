//! `sentinal watch`: create a WATCH / FULL Session and attach this terminal.

use crate::client::{ClientError, DaemonClient};
use protocol::{ClientRequest, DaemonMessage, ProtocolError, Session, SessionOrigin};
use std::{future::Future, path::PathBuf};

/// Environment variable set inside every daemon-owned FULL Session.
pub const SESSION_ENV: &str = "SENTINAL_SESSION_ID";

/// Local facts the Watch command depends on, gathered by the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchContext {
    /// Caller's current working directory.
    pub cwd: PathBuf,
    /// Current terminal size as `(cols, rows)`, if known.
    pub size: Option<(u16, u16)>,
    /// Caller's `$SHELL`, used as the Session's interactive shell.
    pub shell: Option<String>,
    /// Value of `SENTINAL_SESSION_ID` in the caller's environment.
    pub existing_session: Option<String>,
    /// Whether stdin and stdout are an interactive terminal.
    pub interactive: bool,
}

/// What `sentinal watch` should do, decided before contacting the daemon.
#[derive(Debug, Clone, PartialEq)]
pub enum WatchPlan {
    /// This terminal is already inside a Sentinal Session; create nothing.
    AlreadyWatched {
        /// The enclosing Session ID from the environment.
        session: String,
        /// True when a Command was supplied and therefore not run.
        command_refused: bool,
    },
    /// Ask the daemon to create this Session, then attach.
    Create(ClientRequest),
}

/// Watch failures, each guaranteeing the supplied Command did not run.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// The working directory is not valid UTF-8 and cannot be sent.
    #[error("current directory '{}' is not valid UTF-8", .0.display())]
    NonUtf8Cwd(PathBuf),
    /// Watch needs a terminal to attach to.
    #[error("sentinal watch needs an interactive terminal on stdin and stdout")]
    NotATerminal,
    /// The request failed protocol validation.
    #[error("invalid watch request: {0}")]
    Invalid(#[from] ProtocolError),
    /// The daemon could not be reached.
    #[error("cannot reach sentinald: {0}")]
    Unavailable(ClientError),
    /// The daemon refused or failed the request.
    #[error("sentinald rejected the Watch Session: {0}")]
    Rejected(String),
}

/// Decide whether to create a Session, without any I/O.
pub fn plan_watch(
    title: Option<String>,
    command: Option<Vec<String>>,
    context: &WatchContext,
) -> Result<WatchPlan, WatchError> {
    if let Some(session) = context.existing_session.clone() {
        return Ok(WatchPlan::AlreadyWatched {
            session,
            command_refused: command.is_some(),
        });
    }
    let cwd = context
        .cwd
        .to_str()
        .ok_or_else(|| WatchError::NonUtf8Cwd(context.cwd.clone()))?
        .to_string();
    // Some terminals report 0x0; treat that as unknown rather than invalid.
    let (cols, rows) = context
        .size
        .filter(|(cols, rows)| *cols > 0 && *rows > 0)
        .unwrap_or((80, 24));
    let request = ClientRequest::CreateSession {
        shell: context.shell.clone().filter(|shell| !shell.is_empty()),
        cwd: Some(cwd),
        cols,
        rows,
        origin: SessionOrigin::Watch,
        title,
        command,
    };
    request.validate()?;
    if !context.interactive {
        return Err(WatchError::NotATerminal);
    }
    Ok(WatchPlan::Create(request))
}

/// Connect and create the planned Session; the caller then attaches.
pub async fn create_session<F, Fut>(
    connect: F,
    request: ClientRequest,
) -> Result<(DaemonClient, Session), WatchError>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<DaemonClient, ClientError>>,
{
    let mut client = connect().await.map_err(WatchError::Unavailable)?;
    match client.request(request).await {
        Ok(DaemonMessage::SessionUpdated { session }) => Ok((client, session)),
        Ok(DaemonMessage::Error { message }) => Err(WatchError::Rejected(message)),
        Ok(other) => Err(WatchError::Rejected(format!("unexpected reply {other:?}"))),
        Err(error) => Err(WatchError::Unavailable(error)),
    }
}
