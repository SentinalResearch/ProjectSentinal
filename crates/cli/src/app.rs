//! Command dispatch shared by the `sentinal` binary and its `ta` alias.

use crate::{
    args::{AutoAction, Cli, Command, ConnectionAction, IdentityAction},
    client::{terminal_size, AttachEnd, ClientError, DaemonClient},
    connect_command::{self, CodeSource},
    daemon_url::{self, DaemonUrlError},
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
    DaemonUrl(#[from] DaemonUrlError),
    #[error(transparent)]
    Client(#[from] ClientError),
    #[error(transparent)]
    Update(#[from] crate::update::UpdateError),
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
    let cli = match cli.command {
        Command::Doctor { prefix, config_dir } => {
            return Ok(crate::doctor::run(prefix, config_dir, out));
        }
        Command::Setup {
            role,
            name,
            control_url,
            yes,
            code_stdin,
            prefix,
            config_dir,
        } => {
            return Ok(exit_code(
                setup(
                    SetupArgs {
                        role,
                        name,
                        control_url,
                        yes,
                        code_stdin,
                        prefix,
                        config_dir,
                    },
                    out,
                    err,
                )
                .await,
            ))
        }
        Command::Release { action, options } => {
            return Ok(crate::release::run(action, options, out, err));
        }
        Command::Update {
            check,
            rollback,
            prefix,
        } => return Ok(crate::update::run(check, rollback, prefix, out)?),
        Command::Hook { action } => return Ok(crate::hook_command::run(action, out, err).await),
        Command::Auto { action } => {
            let urls = if matches!(&action, AutoAction::Enter { .. }) {
                resolve_daemon_urls(cli.url.as_deref())?
            } else {
                cli.url.into_iter().collect()
            };
            return Ok(crate::auto_command::run(action, &urls, cli.token_file, out, err).await);
        }
        Command::Connect {
            control_url,
            name,
            code_stdin,
        } => {
            return Ok(exit_code(
                connect(&control_url, name.as_deref(), code_stdin, out, err).await,
            ))
        }
        Command::Connection {
            action: ConnectionAction::Status { json },
        } => {
            let Some(socket) = connect_command::default_socket() else {
                return Err(AppError::MissingHome);
            };
            return Ok(exit_code(
                connect_command::status(&socket, json, out, err).await,
            ));
        }
        Command::Disconnect => {
            let Some(socket) = connect_command::default_socket() else {
                return Err(AppError::MissingHome);
            };
            return Ok(exit_code(
                connect_command::disconnect(&socket, out, err).await,
            ));
        }
        Command::Identity {
            action: IdentityAction::Reset { yes },
        } => {
            let Some(socket) = connect_command::default_socket() else {
                return Err(AppError::MissingHome);
            };
            let mut input = std::io::stdin().lock();
            return Ok(exit_code(
                connect_command::identity_reset(&socket, yes, &mut input, out, err).await,
            ));
        }
        Command::Adopt {
            pid,
            pick,
            log,
            title,
        } => {
            let mut input = std::io::stdin().lock();
            return Ok(
                crate::adopt_command::run(pid, pick, log, title, &mut input, out, err).await,
            );
        }
        command => Cli { command, ..cli },
    };
    let token_file = match cli.token_file {
        Some(path) => path,
        None => default_token_file()?,
    };
    let urls = resolve_daemon_urls(cli.url.as_deref())?;
    if let Command::Watch { title, command } = cli.command {
        return watch(&urls, &token_file, title, command, err).await;
    }

    let mut client = DaemonClient::connect(&urls, &token_file).await?;
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
        Command::Top { once } => {
            let live = !once && std::io::stdout().is_terminal();
            loop {
                let text = crate::top::render_top(
                    &crate::top::fetch(&mut client, crate::hook_report::now()).await?,
                );
                if live {
                    // Home and clear, then redraw in place.
                    let _ = write!(
                        out,
                        "\x1b[H\x1b[2J{text}\nRefreshing every 2 s · Ctrl-C to quit\n"
                    );
                } else {
                    let _ = write!(out, "{text}");
                }
                let _ = out.flush();
                if !live {
                    break;
                }
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                    _ = tokio::signal::ctrl_c() => break,
                }
            }
        }
        Command::Status { id } => {
            let snapshot = crate::top::fetch(&mut client, crate::hook_report::now()).await?;
            let Some(session) = snapshot.sessions.iter().find(|session| session.id.0 == id) else {
                let _ = writeln!(err, "sentinal: no Session {id} on this Machine");
                return Ok(ExitCode::FAILURE);
            };
            let events = crate::top::fetch_events(&mut client, session.id).await?;
            let _ = write!(
                out,
                "{}",
                crate::top::render_status(&snapshot, session, &events)
            );
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
        Command::Watch { .. }
        | Command::Doctor { .. }
        | Command::Setup { .. }
        | Command::Release { .. }
        | Command::Update { .. }
        | Command::Hook { .. }
        | Command::Auto { .. }
        | Command::Adopt { .. }
        | Command::Connect { .. }
        | Command::Connection { .. }
        | Command::Disconnect
        | Command::Identity { .. } => {}
    }
    Ok(ExitCode::SUCCESS)
}

async fn watch(
    urls: &[String],
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
                create_session(|| DaemonClient::connect(urls, token_file), request).await?;
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

fn resolve_daemon_urls(explicit: Option<&str>) -> Result<Vec<String>, AppError> {
    if let Some(url) = explicit {
        return Ok(vec![url.to_owned()]);
    }
    let config = std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".config/terminal-agent/daemon.toml"))
        .ok_or(AppError::MissingHome)?;
    Ok(daemon_url::candidates(None, &config)?)
}

fn print_unexpected(err: &mut dyn Write, message: DaemonMessage) {
    let _ = match message {
        DaemonMessage::Error { message } => writeln!(err, "sentinal: error: {message}"),
        other => writeln!(err, "sentinal: unexpected daemon response: {other:?}"),
    };
}

fn exit_code(code: i32) -> ExitCode {
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

/// Read the code from a hidden prompt (terminal required) or `--code-stdin`.
async fn connect(
    control_url: &str,
    name: Option<&str>,
    code_stdin: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let Some(socket) = connect_command::default_socket() else {
        let _ = writeln!(err, "sentinal: HOME is not set; set SENTINAL_SOCKET");
        return connect_command::exit::LOCAL_DAEMON;
    };
    let mut stdin = std::io::stdin().lock();
    if code_stdin {
        let mut unused = NoTerminal;
        return connect_command::connect(
            &socket,
            control_url,
            name,
            CodeSource::Stdin,
            &mut unused,
            &mut stdin,
            out,
            err,
        )
        .await;
    }
    let tty = std::io::stdin()
        .is_terminal()
        .then(connect_command::Tty::open)
        .and_then(Result::ok);
    let Some(mut tty) = tty else {
        let _ = writeln!(
            err,
            "sentinal: the pairing code needs a terminal or --code-stdin"
        );
        return connect_command::exit::USAGE;
    };
    connect_command::connect(
        &socket,
        control_url,
        name,
        CodeSource::Prompt,
        &mut tty,
        &mut stdin,
        out,
        err,
    )
    .await
}

struct SetupArgs {
    role: Option<crate::args::SetupRoleArg>,
    name: Option<String>,
    control_url: Option<String>,
    yes: bool,
    code_stdin: bool,
    prefix: Option<PathBuf>,
    config_dir: Option<PathBuf>,
}

async fn setup(args: SetupArgs, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    use crate::setup::{self, Outcome, RealSystem, SetupOptions, SetupRole};
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        let _ = writeln!(err, "sentinal setup: HOME is not set");
        return setup::exit::CANCELLED;
    };
    let prefix = args.prefix.unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent()?.parent().map(PathBuf::from))
            .unwrap_or_else(|| home.join(".local"))
    });
    let version = std::fs::read_to_string(prefix.join("share/sentinal/release.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.get("version")?.as_str().map(str::to_string))
        .unwrap_or_else(|| crate::version::VERSION.as_str().to_string());
    let options = SetupOptions {
        role: args.role.map(|role| match role {
            crate::args::SetupRoleArg::Control => SetupRole::Control,
            crate::args::SetupRoleArg::Worker => SetupRole::Worker,
        }),
        name: args.name,
        control_url: args.control_url,
        yes: args.yes,
        interactive: std::io::stdin().is_terminal(),
        config_dir: args
            .config_dir
            .unwrap_or_else(|| home.join(".config/terminal-agent")),
        home,
        user: std::env::var("USER").unwrap_or_default(),
        version,
        prefix: prefix.clone(),
    };
    let mut system = RealSystem { prefix };
    let outcome = {
        let mut stdin = std::io::stdin().lock();
        setup::run(options, &mut system, &mut stdin, out, err)
    };
    match outcome {
        Outcome::Exit(code) => code,
        Outcome::Approve {
            control_url,
            control_machine_id,
            control_name,
            name,
        } => {
            let Some(socket) = connect_command::default_socket() else {
                let _ = writeln!(err, "sentinal: HOME is not set; set SENTINAL_SOCKET");
                return connect_command::exit::LOCAL_DAEMON;
            };
            crate::approval_command::approve(
                &socket,
                crate::approval_command::ApprovalTarget {
                    control_url: &control_url,
                    control_name: &control_name,
                    control_machine_id: &control_machine_id,
                },
                name.as_deref(),
                std::time::Duration::from_secs(2),
                async {
                    let _ = tokio::signal::ctrl_c().await;
                },
                out,
                err,
            )
            .await
        }
        Outcome::Connect { control_url, name } => {
            connect(&control_url, name.as_deref(), args.code_stdin, out, err).await
        }
    }
}

/// `--code-stdin` never touches a terminal.
struct NoTerminal;

impl connect_command::HiddenTerminal for NoTerminal {
    fn set_echo(&mut self, _on: bool) -> std::io::Result<()> {
        Ok(())
    }

    fn read_byte(&mut self) -> std::io::Result<Option<u8>> {
        Ok(None)
    }

    fn write_text(&mut self, _text: &str) -> std::io::Result<()> {
        Ok(())
    }
}
