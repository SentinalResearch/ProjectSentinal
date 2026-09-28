//! Command dispatch shared by the `sentinal` binary and its `ta` alias.

use crate::{
    args::{Cli, Command},
    client::{terminal_size, AttachEnd, ClientError, DaemonClient},
    watch::{create_session, plan_watch, WatchContext, WatchError, WatchPlan, SESSION_ENV},
};
use protocol::{ClientRequest, DaemonMessage, SessionId, SessionOrigin};
use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

/// Run one parsed command, writing results to `out` and diagnostics to `err`.
pub async fn run(cli: Cli, out: &mut dyn Write, err: &mut dyn Write) -> ExitCode {
    match dispatch(cli, out, err).await {
        Ok(code) => code,
        Err(error) => {
            let _ = writeln!(err, "sentinal: {error}");
            ExitCode::FAILURE
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("HOME is not set; pass --token-file explicitly")]
    MissingHome,
    #[error("cannot read the current directory: {0}")]
    Cwd(std::io::Error),
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error("{0}; the command was not run outside Sentinal")]
    Watch(#[from] WatchError),
    #[error(
        "created Session {id} but could not attach: {source}; reattach with `sentinal attach {id}`"
    )]
    WatchAttach { id: u64, source: ClientError },
}

async fn dispatch(
    cli: Cli,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<ExitCode, AppError> {
    // Hook commands never use the WebSocket API and need no token.
    if let Command::Hook { action } = cli.command {
        return Ok(crate::hook_command::run(action, out, err).await);
    }
    let token_file = match cli.token_file {
        Some(path) => path,
        None => default_token_file()?,
    };
    let url = cli.url;
    if let Command::Watch { title, command } = cli.command {
        return watch(&url, &token_file, title, command, err).await;
    }

    let mut client = DaemonClient::connect(&url, &token_file).await?;
    match cli.command {
        Command::List => match client.request(ClientRequest::ListSessions).await? {
            DaemonMessage::Sessions { sessions } => {
                for session in sessions {
                    let _ = writeln!(
                        out,
                        "{}\t{:?}\t{}\t{}",
                        session.id.0, session.state, session.cwd, session.shell
                    );
                }
            }
            message => print_unexpected(err, message),
        },
        Command::New { cwd } => {
            let request = ClientRequest::CreateSession {
                shell: None,
                cwd: cwd.map(|path| path.to_string_lossy().to_string()),
                cols: 80,
                rows: 24,
                origin: SessionOrigin::Ui,
                title: None,
                command: None,
            };
            match client.request(request).await? {
                DaemonMessage::SessionUpdated { session } => {
                    let _ = writeln!(out, "{}", session.id.0);
                }
                message => print_unexpected(err, message),
            }
        }
        Command::Attach { id } => {
            let _ = client.attach(SessionId(id)).await?;
        }
        Command::Kill { id } => {
            match client
                .request(ClientRequest::Kill {
                    session_id: SessionId(id),
                })
                .await?
            {
                DaemonMessage::SessionUpdated { session } => {
                    let _ = writeln!(out, "{}\t{:?}", session.id.0, session.state);
                }
                message => print_unexpected(err, message),
            }
        }
        Command::Watch { .. } | Command::Hook { .. } => {}
    }
    Ok(ExitCode::SUCCESS)
}

async fn watch(
    url: &str,
    token_file: &std::path::Path,
    title: Option<String>,
    command: Option<Vec<String>>,
    err: &mut dyn Write,
) -> Result<ExitCode, AppError> {
    let context = WatchContext {
        cwd: std::env::current_dir().map_err(AppError::Cwd)?,
        size: terminal_size(),
        shell: std::env::var("SHELL").ok(),
        existing_session: std::env::var(SESSION_ENV).ok().filter(|id| !id.is_empty()),
        interactive: std::io::stdin().is_terminal() && std::io::stdout().is_terminal(),
    };
    match plan_watch(title, command, &context)? {
        WatchPlan::AlreadyWatched {
            session,
            command_refused: false,
        } => {
            let _ = writeln!(
                err,
                "sentinal: this terminal is already watched by Session {session}; not starting a nested Session"
            );
            Ok(ExitCode::SUCCESS)
        }
        WatchPlan::AlreadyWatched {
            session,
            command_refused: true,
        } => {
            let _ = writeln!(
                err,
                "sentinal: this terminal is already watched by Session {session}; not starting a nested Session and the command was not run (run it directly here)"
            );
            Ok(ExitCode::FAILURE)
        }
        WatchPlan::Create(request) => {
            let (client, session) =
                create_session(|| DaemonClient::connect(url, token_file), request).await?;
            let id = session.id.0;
            let end = client
                .attach(session.id)
                .await
                .map_err(|source| AppError::WatchAttach { id, source })?;
            if end == AttachEnd::Detached {
                let _ = writeln!(
                    err,
                    "sentinal: detached from Session {id}; it keeps running. Reattach with `sentinal attach {id}`"
                );
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

fn default_token_file() -> Result<PathBuf, AppError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config/terminal-agent/token"))
        .ok_or(AppError::MissingHome)
}

fn print_unexpected(err: &mut dyn Write, message: DaemonMessage) {
    let _ = match message {
        DaemonMessage::Error { message } => writeln!(err, "sentinal: error: {message}"),
        other => writeln!(err, "sentinal: unexpected daemon response: {other:?}"),
    };
}
