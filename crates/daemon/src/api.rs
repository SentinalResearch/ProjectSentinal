//! Axum HTTP and WebSocket API for daemon clients.

use crate::{
    auth::constant_time_eq,
    framing::{decode_terminal_frame, encode_terminal_frame},
    session_manager::{Attachment, CreateSession, SessionManager},
};
use axum::{
    extract::{
        connect_info::ConnectInfo,
        ws::{Message, WebSocket, WebSocketUpgrade},
        Request, State,
    },
    http::{header::AUTHORIZATION, StatusCode},
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
    token: String,
}

impl AppState {
    /// Build API state with one bearer token.
    pub fn new(sessions: SessionManager, machine: Machine, token: String) -> Self {
        AppState {
            sessions,
            machine,
            token,
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
            } => {
                let request = CreateSession {
                    shell,
                    cwd: cwd.map(Into::into),
                    cols,
                    rows,
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
            ClientRequest::GetHistory { .. } => WsAction::Reply(DaemonMessage::Error {
                message: "command history is available in Milestone 2".into(),
            }),
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

/// Build the complete versioned HTTP and WebSocket router.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/api/v1/machine", get(machine))
        .route("/api/v1/sessions", get(sessions))
        .route("/api/v1/ws", any(websocket))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize));

    Router::new()
        .route("/api/v1/health", get(health))
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
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let header_token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let query_token = (request.uri().path() == "/api/v1/ws")
        .then(|| request.uri().query())
        .flatten()
        .and_then(token_from_query);
    let supplied = header_token.or(query_token);
    if supplied.is_some_and(|token| constant_time_eq(token, &state.token)) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
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
) -> Response {
    let client_addr = address.to_string();
    upgrade
        .on_upgrade(move |socket| handle_socket(socket, state, client_addr))
        .into_response()
}

async fn handle_socket(mut socket: WebSocket, state: AppState, client_addr: String) {
    let (output_tx, mut output_rx) = mpsc::channel::<Vec<u8>>(256);
    let mut attachments: HashMap<SessionId, JoinHandle<()>> = HashMap::new();

    loop {
        tokio::select! {
            incoming = socket.recv() => {
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
                let Some(frame) = outgoing else { break; };
                if socket.send(Message::Binary(frame.into())).await.is_err() {
                    break;
                }
            }
        }
    }

    for (_, task) in attachments {
        task.abort();
    }
}

async fn handle_client_message(
    socket: &mut WebSocket,
    state: &AppState,
    client_addr: &str,
    output_tx: &mpsc::Sender<Vec<u8>>,
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
    output_tx: &mpsc::Sender<Vec<u8>>,
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
                    match attachment.output.recv().await {
                        Ok(bytes) => {
                            if output_tx
                                .send(encode_terminal_frame(session_id, &bytes))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                            tracing::warn!(
                                session = session_id.0,
                                skipped,
                                "attached client lagged"
                            );
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
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
