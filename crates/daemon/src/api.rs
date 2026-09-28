//! Axum HTTP and WebSocket API for daemon clients.

mod auth_routes;
#[cfg(test)]
mod capture_tests;

use crate::{
    auth::AuthManager,
    framing::{decode_terminal_frame, encode_terminal_frame},
    session_manager::{Attachment, CreateSession, SessionManager},
};
use axum::{
    extract::{
        connect_info::ConnectInfo,
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Extension, Request, State,
    },
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get},
    Json, Router,
};
use protocol::{ClientRequest, DaemonMessage, Envelope, Machine, SessionId, PROTOCOL_VERSION};
use serde::Serialize;
use std::{collections::HashMap, net::SocketAddr};
use tokio::{sync::mpsc, task::JoinHandle};

/// Shared state used by every daemon API route.
#[derive(Clone)]
pub struct AppState {
    /// Persistent sessions controlled by this daemon.
    pub sessions: SessionManager,
    /// Description returned by the machine endpoint.
    pub machine: Machine,
    auth: AuthManager,
}

impl AppState {
    /// Build API state with persistent single-user authentication.
    pub fn new(sessions: SessionManager, machine: Machine, auth: AuthManager) -> Self {
        AppState {
            sessions,
            machine,
            auth,
        }
    }

    /// Decode, validate, and dispatch one WebSocket text frame.
    pub async fn dispatch_text(&self, text: &str) -> WsAction {
        let request = match Envelope::<ClientRequest>::decode(text) {
            Ok(envelope) => envelope.payload,
            Err(error) => return error_action(error),
        };
        if let Err(error) = request.validate() {
            return error_action(error);
        }

        match request {
            ClientRequest::GetMachine => WsAction::Reply(DaemonMessage::Machine {
                machine: self.machine.clone(),
            }),
            ClientRequest::ListSessions => match self.sessions.list() {
                Ok(sessions) => WsAction::Reply(DaemonMessage::Sessions { sessions }),
                Err(error) => error_action(error),
            },
            ClientRequest::CreateSession {
                shell,
                cwd,
                cols,
                rows,
                origin,
                title,
                command,
            } => {
                let request = CreateSession {
                    shell,
                    cwd: cwd.map(Into::into),
                    cols,
                    rows,
                    origin,
                    title,
                    command,
                };
                match self.sessions.create(request).await {
                    Ok(session) => WsAction::Reply(DaemonMessage::SessionUpdated { session }),
                    Err(error) => error_action(error),
                }
            }
            ClientRequest::Attach { session_id } => match self.sessions.attach(session_id) {
                Ok(attachment) => WsAction::Attach {
                    session_id,
                    attachment,
                },
                Err(error) => error_action(error),
            },
            ClientRequest::Detach { session_id } => WsAction::Detach(session_id),
            ClientRequest::Resize {
                session_id,
                cols,
                rows,
            } => match self.sessions.resize(session_id, cols, rows) {
                Ok(()) => session_reply(&self.sessions, session_id),
                Err(error) => error_action(error),
            },
            ClientRequest::Kill { session_id } => match self.sessions.kill(session_id) {
                Ok(()) => session_reply(&self.sessions, session_id),
                Err(error) => error_action(error),
            },
            ClientRequest::GetHistory { session_id, limit } => {
                match self.sessions.history(session_id, limit.min(1_000)) {
                    Ok(commands) => WsAction::Reply(DaemonMessage::History { commands }),
                    Err(error) => error_action(error),
                }
            }
            ClientRequest::SearchCommands { query, limit } => {
                match self.sessions.search_commands(&query, limit.min(1_000)) {
                    Ok(commands) => WsAction::Reply(DaemonMessage::History { commands }),
                    Err(error) => error_action(error),
                }
            }
        }
    }
}

/// Side effect requested by one valid or invalid WebSocket control message.
pub enum WsAction {
    /// Send one structured daemon reply.
    Reply(DaemonMessage),
    /// Send a screen snapshot and begin forwarding live output.
    Attach {
        /// Session being attached.
        session_id: SessionId,
        /// Snapshot and output subscription.
        attachment: Attachment,
    },
    /// Stop forwarding one session to this client.
    Detach(SessionId),
}

#[derive(Serialize)]
struct Health {
    ok: bool,
    version: u32,
}

#[derive(Clone)]
struct AuthorizedToken(String);

/// Build the complete versioned HTTP and WebSocket router.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/v1/machine", get(machine))
        .route("/api/v1/sessions", get(sessions))
        .route("/api/v1/ws", any(websocket))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize));

    Router::new()
        .route("/api/v1/health", get(health))
        .merge(auth_routes::routes())
        .merge(protected)
        .with_state(state)
}

async fn health() -> Json<Health> {
    Json(Health {
        ok: true,
        version: PROTOCOL_VERSION,
    })
}

async fn machine(State(state): State<AppState>) -> Json<Envelope<DaemonMessage>> {
    Json(Envelope::new(DaemonMessage::Machine {
        machine: state.machine,
    }))
}

async fn sessions(State(state): State<AppState>) -> Json<Envelope<DaemonMessage>> {
    let message = match state.sessions.list() {
        Ok(sessions) => DaemonMessage::Sessions { sessions },
        Err(error) => DaemonMessage::Error {
            message: error.to_string(),
        },
    };
    Json(Envelope::new(message))
}

async fn authorize(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let header_token = auth_routes::bearer_from_headers(request.headers());
    let query_token = (request.uri().path() == "/api/v1/ws")
        .then(|| request.uri().query())
        .flatten()
        .and_then(token_from_query);
    let supplied = header_token.or(query_token).map(str::to_string);
    match supplied
        .as_deref()
        .map(|token| state.auth.authorize(token))
        .transpose()
    {
        Ok(Some(Some(_))) => {
            if let Some(token) = supplied {
                request.extensions_mut().insert(AuthorizedToken(token));
            }
            Ok(next.run(request).await)
        }
        Ok(_) => Err(StatusCode::UNAUTHORIZED),
        Err(error) => {
            tracing::error!(%error, "authentication state failed");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}

fn token_from_query(query: &str) -> Option<&str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
}

async fn websocket(
    upgrade: WebSocketUpgrade,
    State(state): State<AppState>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    Extension(token): Extension<AuthorizedToken>,
) -> Response {
    let client_addr = address.to_string();
    upgrade
        .on_upgrade(move |socket| handle_socket(socket, state, client_addr, token.0))
        .into_response()
}

async fn handle_socket(mut socket: WebSocket, state: AppState, client_addr: String, token: String) {
    let (output_tx, mut output_rx) = mpsc::channel::<AttachedOutput>(256);
    let mut attachments: HashMap<SessionId, JoinHandle<()>> = HashMap::new();

    loop {
        tokio::select! {
            incoming = socket.recv() => {
                if !connection_authorized(&state, &token) {
                    close_expired(&mut socket).await;
                    break;
                }
                let Some(incoming) = incoming else { break; };
                let message = match incoming {
                    Ok(message) => message,
                    Err(error) => {
                        tracing::debug!(%client_addr, %error, "websocket client disconnected");
                        break;
                    }
                };
                if !handle_client_message(
                    &mut socket,
                    &state,
                    &client_addr,
                    &output_tx,
                    &mut attachments,
                    message,
                ).await {
                    break;
                }
            }
            outgoing = output_rx.recv() => {
                if !connection_authorized(&state, &token) {
                    close_expired(&mut socket).await;
                    break;
                }
                let Some(frame) = outgoing else { break; };
                let sent = match frame {
                    AttachedOutput::Binary(bytes) => socket.send(Message::Binary(bytes.into())).await.is_ok(),
                    AttachedOutput::Structured(message) => send_daemon_message(&mut socket, message).await,
                };
                if !sent {
                    break;
                }
            }
        }
    }

    for (_, task) in attachments {
        task.abort();
    }
}

async fn close_expired(socket: &mut WebSocket) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: 4_001,
            reason: "browser session expired".into(),
        })))
        .await;
}

fn connection_authorized(state: &AppState, token: &str) -> bool {
    match state.auth.authorize(token) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(error) => {
            tracing::error!(%error, "failed to revalidate websocket authentication");
            false
        }
    }
}

async fn handle_client_message(
    socket: &mut WebSocket,
    state: &AppState,
    client_addr: &str,
    output_tx: &mpsc::Sender<AttachedOutput>,
    attachments: &mut HashMap<SessionId, JoinHandle<()>>,
    message: Message,
) -> bool {
    match message {
        Message::Text(text) => {
            let action = state.dispatch_text(text.as_str()).await;
            apply_action(socket, output_tx, attachments, action).await
        }
        Message::Binary(frame) => {
            let result = decode_terminal_frame(&frame)
                .map_err(|error| error.to_string())
                .and_then(|(session_id, bytes)| {
                    state
                        .sessions
                        .write_input(session_id, bytes, client_addr)
                        .map_err(|error| error.to_string())
                });
            if let Err(message) = result {
                return send_daemon_message(socket, DaemonMessage::Error { message }).await;
            }
            true
        }
        Message::Ping(bytes) => socket.send(Message::Pong(bytes)).await.is_ok(),
        Message::Close(_) => false,
        Message::Pong(_) => true,
    }
}

async fn apply_action(
    socket: &mut WebSocket,
    output_tx: &mpsc::Sender<AttachedOutput>,
    attachments: &mut HashMap<SessionId, JoinHandle<()>>,
    action: WsAction,
) -> bool {
    match action {
        WsAction::Reply(message) => send_daemon_message(socket, message).await,
        WsAction::Attach {
            session_id,
            mut attachment,
        } => {
            if socket
                .send(Message::Binary(
                    encode_terminal_frame(session_id, &attachment.screen).into(),
                ))
                .await
                .is_err()
            {
                return false;
            }
            if let Some(previous) = attachments.remove(&session_id) {
                previous.abort();
            }
            let output_tx = output_tx.clone();
            let task = tokio::spawn(async move {
                loop {
                    tokio::select! {
                        output = attachment.output.recv() => match output {
                            Ok(bytes) => {
                                if output_tx.send(AttachedOutput::Binary(
                                    encode_terminal_frame(session_id, &bytes),
                                )).await.is_err() {
                                    return;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(session = session_id.0, skipped, "attached terminal client lagged");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        },
                        structured = attachment.structured.recv() => match structured {
                            Ok(message) => {
                                if output_tx.send(AttachedOutput::Structured(message)).await.is_err() {
                                    return;
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(session = session_id.0, skipped, "attached structured client lagged");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
                        },
                    }
                }
            });
            attachments.insert(session_id, task);
            true
        }
        WsAction::Detach(session_id) => {
            if let Some(task) = attachments.remove(&session_id) {
                task.abort();
            }
            true
        }
    }
}

enum AttachedOutput {
    Binary(Vec<u8>),
    Structured(DaemonMessage),
}

async fn send_daemon_message(socket: &mut WebSocket, message: DaemonMessage) -> bool {
    let text = match Envelope::new(message).encode() {
        Ok(text) => text,
        Err(error) => {
            tracing::error!(%error, "failed to encode daemon message");
            return false;
        }
    };
    socket.send(Message::Text(text.into())).await.is_ok()
}

fn session_reply(sessions: &SessionManager, session_id: SessionId) -> WsAction {
    match sessions.session(session_id) {
        Ok(session) => WsAction::Reply(DaemonMessage::SessionUpdated { session }),
        Err(error) => error_action(error),
    }
}

fn error_action(error: impl std::fmt::Display) -> WsAction {
    WsAction::Reply(DaemonMessage::Error {
        message: error.to_string(),
    })
}
