//! The terminal-agent command-line client.

use clap::Parser;
use cli::{
    args::{Cli, Command},
    client::{ClientError, DaemonClient},
};
use protocol::{ClientRequest, DaemonMessage, SessionId};
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
enum MainError {
    #[error("HOME is not set; pass --token-file explicitly")]
    MissingHome,
    #[error(transparent)]
    Client(#[from] ClientError),
}

#[tokio::main]
async fn main() -> Result<(), MainError> {
    let cli = Cli::parse();
    let token_file = match cli.token_file {
        Some(path) => path,
        None => default_token_file()?,
    };
    let mut client = DaemonClient::connect(&cli.url, &token_file).await?;

    match cli.command {
        Command::List => match client.request(ClientRequest::ListSessions).await? {
            DaemonMessage::Sessions { sessions } => {
                for session in sessions {
                    println!(
                        "{}\t{:?}\t{}\t{}",
                        session.id.0, session.state, session.cwd, session.shell
                    );
                }
            }
            message => print_unexpected(message),
        },
        Command::New { cwd } => {
            let request = ClientRequest::CreateSession {
                shell: None,
                cwd: cwd.map(|path| path.to_string_lossy().to_string()),
                cols: 80,
                rows: 24,
            };
            match client.request(request).await? {
                DaemonMessage::SessionUpdated { session } => println!("{}", session.id.0),
                message => print_unexpected(message),
            }
        }
        Command::Attach { id } => client.attach(SessionId(id)).await?,
        Command::Kill { id } => {
            match client
                .request(ClientRequest::Kill {
                    session_id: SessionId(id),
                })
                .await?
            {
                DaemonMessage::SessionUpdated { session } => {
                    println!("{}\t{:?}", session.id.0, session.state);
                }
                message => print_unexpected(message),
            }
        }
    }
    Ok(())
}

fn default_token_file() -> Result<PathBuf, MainError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config/terminal-agent/token"))
        .ok_or(MainError::MissingHome)
}

fn print_unexpected(message: DaemonMessage) {
    match message {
        DaemonMessage::Error { message } => eprintln!("error: {message}"),
        other => eprintln!("unexpected daemon response: {other:?}"),
    }
}
