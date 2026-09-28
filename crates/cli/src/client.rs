//! WebSocket client transport and interactive terminal attachment.

use crate::{
    detach::DetachDetector,
    frame::{decode_terminal_frame, encode_terminal_frame},
};
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientRequest, DaemonMessage, Envelope, ProtocolError, SessionId};
use std::{
    path::PathBuf,
    process::{Command, Stdio},
};
use tokio::io::AsyncWriteExt;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{
        client::IntoClientRequest,
        http::{header::AUTHORIZATION, HeaderValue},
        Error as WebSocketError, Message,
    },
    MaybeTlsStream, WebSocketStream,
};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Errors returned by the CLI transport or local terminal setup.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// A token could not be read.
    #[error("failed to read token file '{}': {source}", path.display())]
    TokenFile {
        /// Token path.
        path: PathBuf,
        /// Filesystem error.
        source: std::io::Error,
    },
    /// A WebSocket request could not be constructed.
    #[error("invalid daemon WebSocket URL '{url}': {message}")]
    Url {
        /// Invalid URL.
        url: String,
        /// Parser error.
        message: String,
    },
    /// The bearer token was not a valid HTTP header value.
    #[error("invalid bearer token from '{}': {message}", path.display())]
    TokenHeader {
        /// Token path.
        path: PathBuf,
        /// Header parser error.
        message: String,
    },
    /// A WebSocket operation failed.
    #[error("daemon WebSocket failed: {0}")]
    WebSocket(#[from] WebSocketError),
    /// A protocol message could not be encoded or decoded.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// The daemon returned an application error.
    #[error("daemon rejected request: {0}")]
    Daemon(String),
    /// The daemon disconnected before replying.
    #[error("daemon disconnected before replying")]
    Disconnected,
    /// A local terminal operation failed.
    #[error("failed to {operation} local terminal: {message}")]
    Terminal {
        /// Operation that failed.
        operation: &'static str,
        /// Error details.
        message: String,
    },
    /// Local standard input or output failed.
    #[error("failed to {operation} terminal bytes: {source}")]
    Io {
        /// Operation that failed.
        operation: &'static str,
        /// I/O error.
        source: std::io::Error,
    },
}

/// Whether `message` is an unsolicited observation (Metrics, Events,
/// Command updates, or a reduced-capture Session update) rather than a
/// reply. Replies to requests are never any of these.
pub fn is_observation(message: &DaemonMessage) -> bool {
    match message {
        DaemonMessage::SessionMetrics { .. }
        | DaemonMessage::MachineMetrics { .. }
        | DaemonMessage::Event { .. }
        | DaemonMessage::CommandUpdated { .. } => true,
        DaemonMessage::SessionUpdated { session } => {
            session.capture != protocol::CaptureLevel::Full
        }
        _ => false,
    }
}

/// Why an interactive attachment ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachEnd {
    /// The user detached (Ctrl-\\ d) or input reached EOF; the Session keeps running.
    Detached,
    /// The Session's shell exited.
    Ended {
        /// Shell exit code, when known.
        exit_code: Option<i32>,
    },
}

/// One authenticated WebSocket connection to a daemon.
pub struct DaemonClient {
    socket: Socket,
}

impl std::fmt::Debug for DaemonClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DaemonClient")
            .finish_non_exhaustive()
    }
}

impl DaemonClient {
    /// Connect using the bearer token stored at `token_path`.
    pub async fn connect(url: &str, token_path: &std::path::Path) -> Result<Self, ClientError> {
        let token = std::fs::read_to_string(token_path)
            .map_err(|source| ClientError::TokenFile {
                path: token_path.to_path_buf(),
                source,
            })?
            .trim()
            .to_string();
        let mut request = url
            .into_client_request()
            .map_err(|error| ClientError::Url {
                url: url.to_string(),
                message: error.to_string(),
            })?;
        let header = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|error| {
            ClientError::TokenHeader {
                path: token_path.to_path_buf(),
                message: error.to_string(),
            }
        })?;
        request.headers_mut().insert(AUTHORIZATION, header);
        let (socket, _) = connect_async(request).await?;
        Ok(DaemonClient { socket })
    }

    /// Send one request and wait for the next structured daemon response.
    pub async fn request(&mut self, request: ClientRequest) -> Result<DaemonMessage, ClientError> {
        self.send_control(request).await?;
        while let Some(message) = self.socket.next().await {
            match message? {
                Message::Text(text) => {
                    let message = Envelope::<DaemonMessage>::decode(text.as_str())?.payload;
                    if !is_observation(&message) {
                        return Ok(message);
                    }
                }
                Message::Close(_) => return Err(ClientError::Disconnected),
                _ => {}
            }
        }
        Err(ClientError::Disconnected)
    }

    /// Attach this process's terminal until `Ctrl-\ d`, EOF, or the shell exits.
    pub async fn attach(mut self, session_id: SessionId) -> Result<AttachEnd, ClientError> {
        self.send_control(ClientRequest::Attach { session_id })
            .await?;
        if let Some((cols, rows)) = terminal_size() {
            self.send_control(ClientRequest::Resize {
                session_id,
                cols,
                rows,
            })
            .await?;
        }
        let _raw = RawTerminal::enable()?;
        let mut stdin = spawn_stdin_reader();
        let mut stdout = tokio::io::stdout();
        let mut detector = DetachDetector::new();

        loop {
            tokio::select! {
                chunk = stdin.recv() => {
                    let input = match chunk {
                        Some(Ok(bytes)) => bytes,
                        Some(Err(source)) => {
                            return Err(ClientError::Io {
                                operation: "read",
                                source,
                            });
                        }
                        None => Vec::new(),
                    };
                    let count = input.len();
                    if count == 0 {
                        let trailing = detector.finish();
                        if !trailing.is_empty() {
                            self.socket.send(Message::Binary(
                                encode_terminal_frame(session_id, &trailing).into()
                            )).await?;
                        }
                        return Ok(AttachEnd::Detached);
                    }
                    let result = detector.push(&input);
                    if !result.bytes.is_empty() {
                        self.socket.send(Message::Binary(
                            encode_terminal_frame(session_id, &result.bytes).into()
                        )).await?;
                    }
                    if result.detach {
                        self.send_control(ClientRequest::Detach { session_id }).await?;
                        return Ok(AttachEnd::Detached);
                    }
                }
                message = self.socket.next() => {
                    let Some(message) = message else {
                        return Err(ClientError::Disconnected);
                    };
                    match message? {
                        Message::Binary(frame) => {
                            let (frame_session, bytes) = decode_terminal_frame(&frame)
                                .map_err(|error| ClientError::Daemon(error.to_string()))?;
                            if frame_session == session_id {
                                stdout.write_all(bytes).await.map_err(|source| ClientError::Io {
                                    operation: "write",
                                    source,
                                })?;
                                stdout.flush().await.map_err(|source| ClientError::Io {
                                    operation: "flush",
                                    source,
                                })?;
                            }
                        }
                        Message::Text(text) => {
                            let message = Envelope::<DaemonMessage>::decode(text.as_str())?.payload;
                            match message {
                                DaemonMessage::Error { message } => {
                                    return Err(ClientError::Daemon(message));
                                }
                                // The shell exited: there is nothing left to show.
                                DaemonMessage::SessionUpdated { session }
                                    if session.id == session_id && session.state.is_finished() =>
                                {
                                    return Ok(AttachEnd::Ended { exit_code: session.exit_code });
                                }
                                _ => {}
                            }
                        }
                        Message::Close(_) => return Err(ClientError::Disconnected),
                        _ => {}
                    }
                }
            }
        }
    }

    async fn send_control(&mut self, request: ClientRequest) -> Result<(), ClientError> {
        let text = Envelope::new(request).encode()?;
        self.socket.send(Message::Text(text.into())).await?;
        Ok(())
    }
}

/// Read terminal input on a plain thread. Unlike `tokio::io::stdin`, a read
/// still pending when the shell ends does not keep the process alive: std
/// threads do not block process exit, while Tokio's blocking pool does.
fn spawn_stdin_reader() -> tokio::sync::mpsc::Receiver<std::io::Result<Vec<u8>>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(16);
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buffer = [0_u8; 4096];
        loop {
            let result =
                std::io::Read::read(&mut stdin, &mut buffer).map(|count| buffer[..count].to_vec());
            let finished = !matches!(&result, Ok(bytes) if !bytes.is_empty());
            if sender.blocking_send(result).is_err() || finished {
                return;
            }
        }
    });
    receiver
}

struct RawTerminal {
    previous: String,
}

impl RawTerminal {
    fn enable() -> Result<Self, ClientError> {
        let output = Command::new("stty")
            .arg("-g")
            .stdin(Stdio::inherit())
            .output()
            .map_err(|error| terminal_error("read mode", error))?;
        if !output.status.success() {
            return Err(ClientError::Terminal {
                operation: "read mode",
                message: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        let previous = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let status = Command::new("stty")
            .args(["raw", "-echo"])
            .status()
            .map_err(|error| terminal_error("enable raw mode", error))?;
        if !status.success() {
            return Err(ClientError::Terminal {
                operation: "enable raw mode",
                message: format!("stty exited with {status}"),
            });
        }
        Ok(RawTerminal { previous })
    }
}

impl Drop for RawTerminal {
    fn drop(&mut self) {
        let _ = Command::new("stty").arg(&self.previous).status();
    }
}

/// Current terminal size as `(cols, rows)`, or `None` when unknown or 0x0.
pub fn terminal_size() -> Option<(u16, u16)> {
    let output = Command::new("stty")
        .arg("size")
        .stdin(Stdio::inherit())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut values = text.split_whitespace();
    let rows: u16 = values.next()?.parse().ok()?;
    let cols: u16 = values.next()?.parse().ok()?;
    // A 0x0 report means the size is unknown; never send it as a resize.
    (cols > 0 && rows > 0).then_some((cols, rows))
}

fn terminal_error(operation: &'static str, error: std::io::Error) -> ClientError {
    ClientError::Terminal {
        operation,
        message: error.to_string(),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::RawTerminal;
    use std::io::IsTerminal;

    #[test]
    #[ignore = "requires a real controlling terminal"]
    fn raw_terminal_integration() {
        if !std::io::stdin().is_terminal() {
            eprintln!("raw_terminal_integration requires a controlling terminal");
            return;
        }
        let terminal = RawTerminal::enable().unwrap();
        drop(terminal);
    }
}
