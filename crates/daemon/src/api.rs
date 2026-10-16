//! Axum HTTP and WebSocket API for daemon clients.

pub mod auth_routes;
#[cfg(test)]
mod capture_tests;
mod output;

use crate::insights::MachineInsights;
use crate::{
    auth::AuthManager,
    control::{routes::ControlRoutes, ControlService},
    framing::{decode_terminal_frame, encode_terminal_frame},
    link::LinkManager,
    notifications::NotificationService,
    origin::OriginPolicy,
    push::gateway::PushGateway,
    recipes::RecipeCatalog,
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
    control: Option<ControlService>,
    link: Option<LinkManager>,
    origins: OriginPolicy,
    behind_tailscale_serve: bool,
    development: bool,
    recipes: RecipeCatalog,
    notifications: Option<NotificationService>,
    push: Option<PushGateway>,
    release: Option<crate::control::ControlRelease>,
    insights: Option<MachineInsights>,
    worker_updater: Option<crate::worker_update::WorkerUpdater>,
}

impl AppState {
    /// Build API state with persistent single-user authentication.
    ///
    /// No browser origin is allowed until [`AppState::with_origins`] says so.
    pub fn new(sessions: SessionManager, machine: Machine, auth: AuthManager) -> Self {
        let (recipes, _) = RecipeCatalog::load(None);
        AppState {
            sessions,
            machine,
            auth,
            control: None,
            link: None,
            origins: OriginPolicy::new(&[], false)
                .unwrap_or_else(|_| unreachable!("empty list is valid")),
            behind_tailscale_serve: false,
            development: false,
            recipes,
            notifications: None,
            push: None,
            release: None,
            insights: None,
            worker_updater: None,
        }
    }

    /// Exact browser origins allowed on every route.
    pub fn with_origins(mut self, origins: OriginPolicy) -> Self {
        self.origins = origins;
        self
    }

    /// Enable the control role.
    pub fn with_control(mut self, control: Option<ControlService>) -> Self {
        self.control = control;
        self
    }

    /// Let the control start this worker's self-update (4.E §11).
    pub fn with_worker_updater(mut self, updater: crate::worker_update::WorkerUpdater) -> Self {
        self.worker_updater = Some(updater);
        self
    }

    /// Attach this daemon's worker link.
    pub fn with_link(mut self, link: LinkManager) -> Self {
        self.link = Some(link);
        self
    }

    /// Trust `Tailscale-User-Login` from loopback peers (Tailscale Serve).
    pub fn with_tailscale_serve(mut self, enabled: bool) -> Self {
        self.behind_tailscale_serve = enabled;
        self
    }

    /// Enable development-only authentication compatibility such as query tokens.
    pub fn with_development(mut self, enabled: bool) -> Self {
        self.development = enabled;
        self
    }

    /// Install the Recipe catalog loaded for this daemon.
    pub fn with_recipes(mut self, recipes: RecipeCatalog) -> Self {
        self.recipes = recipes;
        self
    }

    /// Install the Machine page's host facts, process trees, and history.
    pub fn with_insights(mut self, insights: MachineInsights) -> Self {
        self.insights = Some(insights);
        self
    }

    /// Install local notification rules and delivery for this daemon.
    pub fn with_notifications(mut self, notifications: NotificationService) -> Self {
        self.notifications = Some(notifications);
        self
    }

    /// Install the control-owned native Push gateway.
    pub fn with_push(mut self, push: Option<PushGateway>) -> Self {
        self.push = push;
        self
    }

    /// Install the control's owner-signed release service.
    pub fn with_release(mut self, release: Option<crate::control::ControlRelease>) -> Self {
        self.release = release;
        self
    }

    /// Who is logging in: the Tailscale user behind Serve, else the peer IP.
    pub(crate) fn client_key(
        &self,
        headers: &axum::http::HeaderMap,
        address: SocketAddr,
    ) -> String {
        if self.behind_tailscale_serve && address.ip().is_loopback() {
            if let Some(login) = headers
                .get("tailscale-user-login")
                .and_then(|value| value.to_str().ok())
                .filter(|login| !login.is_empty() && login.len() <= 256)
            {
                return format!("tailscale:{login}");
            }
        }
        address.ip().to_string()
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
            ClientRequest::ContinueSession { session_id } => {
                match self.sessions.continue_session(session_id).await {
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
                Ok(()) => match session_reply(&self.sessions, session_id) {
                    WsAction::Reply(message) => WsAction::Resize {
                        session_id,
                        message,
                    },
                    action => action,
                },
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
            ClientRequest::GetEvents {
                session_id,
                after,
                limit,
            } => match self.sessions.events(session_id, after, limit.min(500)) {
                Ok(events) => WsAction::Reply(DaemonMessage::Events { events }),
                Err(error) => error_action(error),
            },
            ClientRequest::SearchCommands { query, limit } => {
                match self.sessions.search_commands(&query, limit.min(1_000)) {
                    Ok(commands) => WsAction::Reply(DaemonMessage::History { commands }),
                    Err(error) => error_action(error),
                }
            }
            ClientRequest::ListRecipes => WsAction::Reply(DaemonMessage::Recipes {
                recipes: self.recipes.list(),
            }),
            ClientRequest::RunRecipe {
                recipe_id,
                values,
                session_id,
            } => {
                let recipe = match self.recipes.get(&recipe_id) {
                    Ok(recipe) => recipe,
                    Err(error) => return error_action(error),
                };
                if recipe
                    .machine_id
                    .as_ref()
                    .is_some_and(|id| id != &self.machine.id)
                {
                    return error_action(format!(
                        "recipe '{}' is restricted to Machine '{}'",
                        recipe.id,
                        recipe.machine_id.as_ref().map_or("", |id| id.0.as_str())
                    ));
                }
                let rendered = match recipe.render(&values) {
                    Ok(rendered) => rendered,
                    Err(error) => return error_action(error),
                };
                if let Some(session_id) = session_id {
                    let mut bytes = rendered.into_bytes();
                    bytes.push(b'\r');
                    match self.sessions.write_input(session_id, &bytes, "recipe") {
                        Ok(()) => session_reply(&self.sessions, session_id),
                        Err(error) => error_action(error),
                    }
                } else {
                    let cwd = recipe.project.as_ref().map(Into::into);
                    match self
                        .sessions
                        .create_recipe(rendered, cwd, recipe.name.clone())
                        .await
                    {
                        Ok(session) => WsAction::Reply(DaemonMessage::SessionUpdated { session }),
                        Err(error) => error_action(error),
                    }
                }
            }
            ClientRequest::SaveRecipe { recipe } => match self.recipes.save(recipe) {
                Ok(()) => WsAction::Reply(DaemonMessage::Recipes {
                    recipes: self.recipes.list(),
                }),
                Err(error) => error_action(error),
            },
            ClientRequest::GetNotificationRules => match &self.notifications {
                Some(notifications) => WsAction::Reply(DaemonMessage::NotificationRules {
                    rules: notifications.rules(),
                    status: notifications.status(),
                }),
                None => error_action("notifications are not configured"),
            },
            ClientRequest::SetNotificationRule { session_id, rules } => match &self.notifications {
                Some(notifications) => match notifications.set_rule(session_id, rules) {
                    Ok(()) => WsAction::Reply(DaemonMessage::NotificationRules {
                        rules: notifications.rules(),
                        status: notifications.status(),
                    }),
                    Err(error) => error_action(error),
                },
                None => error_action("notifications are not configured"),
            },
            ClientRequest::GetMachineDetails => match &self.insights {
                Some(insights) => WsAction::Reply(DaemonMessage::MachineDetails {
                    details: insights
                        .details(self.link.as_ref().and_then(|link| link.status().ok())),
                }),
                None => error_action("Machine details are not available on this daemon"),
            },
            ClientRequest::GetMachineMetrics { range } => match &self.insights {
                Some(insights) => match insights.history(range, current_time()) {
                    Ok(points) => {
                        WsAction::Reply(DaemonMessage::MachineMetricsHistory { range, points })
                    }
                    Err(error) => error_action(error),
                },
                None => error_action("Machine metrics history is not available on this daemon"),
            },
            ClientRequest::GetNotificationJournal => match &self.notifications {
                Some(notifications) => WsAction::Reply(DaemonMessage::NotificationJournal {
                    entries: notifications.journal(),
                }),
                None => error_action("notifications are not configured"),
            },
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
    /// Confirm a delivered attachment and send the resize response.
    Resize {
        /// Session whose snapshot the client acknowledged.
        session_id: SessionId,
        /// Updated Session response.
        message: DaemonMessage,
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
        .route("/api/v1/sessions/{session_id}/output", get(output::output))
        .route("/api/v1/ws", any(websocket))
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize));

    let control = ControlRoutes::new(
        state.control.clone(),
        state.auth.clone(),
        state.behind_tailscale_serve,
        state.development,
    )
    .with_push(state.push.clone())
    .with_release(state.release.clone())
    .router();
    let origins = state.origins.clone();
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/link/identity", get(link_identity))
        .route("/api/v1/pairing/proof/{request_id}", get(pairing_proof))
        .route("/api/v1/link/update", axum::routing::post(link_update))
        .route("/api/v1/link/move", axum::routing::post(link_move))
        .merge(auth_routes::routes())
        .merge(protected)
        .with_state(state)
        .merge(control)
        .layer(middleware::from_fn_with_state(
            origins,
            crate::origin::enforce,
        ))
}

/// The control asks this worker to update itself; only the control's
/// credential for this link is accepted.
async fn link_update(State(state): State<AppState>, request: Request) -> Response {
    let authorized = state.link.as_ref().is_some_and(|link| {
        auth_routes::bearer_from_headers(request.headers())
            .and_then(|bearer| link.verify_control_credential(bearer))
            .is_some()
    });
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(updater) = state.worker_updater.as_ref() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let body = match axum::body::to_bytes(request.into_body(), 4096).await {
        Ok(body) => body,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let Ok(update) = serde_json::from_slice::<protocol::worker_update::WorkerUpdateRequest>(&body)
    else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match updater.start(&update) {
        Ok(status) => (StatusCode::ACCEPTED, Json(status)).into_response(),
        Err((status, failure)) => (
            StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(failure),
        )
            .into_response(),
    }
}

/// The current control hands this worker to another control (it is stepping
/// down). Only this link's control credential is accepted.
async fn link_move(State(state): State<AppState>, request: Request) -> Response {
    let Some(link) = state.link.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let authorized = auth_routes::bearer_from_headers(request.headers())
        .and_then(|bearer| link.verify_control_credential(bearer))
        .is_some();
    if !authorized {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let body = match axum::body::to_bytes(request.into_body(), 4096).await {
        Ok(body) => body,
        Err(_) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
    };
    let Ok(target) = serde_json::from_slice::<protocol::pairing::LinkMoveRequest>(&body) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    match link.begin_move(target) {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(message) => (StatusCode::BAD_REQUEST, message).into_response(),
    }
}

/// Approval pairing callback: proof that this worker made the open request.
async fn pairing_proof(
    State(state): State<AppState>,
    axum::extract::Path(request_id): axum::extract::Path<String>,
) -> Response {
    match state
        .link
        .as_ref()
        .and_then(|link| link.pairing_proof(&request_id))
    {
        Some(proof) => Json(proof).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// The control proves its link: its bearer must hash to the stored value.
async fn link_identity(State(state): State<AppState>, request: Request) -> Response {
    let identity = state.link.as_ref().and_then(|link| {
        auth_routes::bearer_from_headers(request.headers())
            .and_then(|bearer| link.verify_control_credential(bearer))
    });
    match identity {
        Some(identity) => Json(identity).into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
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
    let credential = auth_routes::credential_from_headers(request.headers(), state.development);
    let query_token = (state.development && request.uri().path() == "/api/v1/ws")
        .then(|| request.uri().query())
        .flatten()
        .and_then(token_from_query);
    let supplied = credential.or_else(|| query_token.map(str::to_string));
    match supplied.as_deref() {
        Some(token) if api_token_authorized(&state, token) => {
            if let Some(token) = supplied {
                request.extensions_mut().insert(AuthorizedToken(token));
            }
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

fn api_token_authorized(state: &AppState, token: &str) -> bool {
    match state.auth.authorize(token) {
        Ok(Some(_)) => true,
        Ok(None) => state
            .link
            .as_ref()
            .and_then(|link| link.verify_control_credential(token))
            .is_some(),
        Err(error) => {
            tracing::error!(%error, "authentication state failed");
            false
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
    let mut attachments: HashMap<SessionId, AttachedForwarder> = HashMap::new();
    let mut observations = state.sessions.observe();

    loop {
        tokio::select! {
            observed = observations.recv() => {
                match observed {
                    Ok(message) => {
                        if !connection_authorized(&state, &token) {
                            close_expired(&mut socket).await;
                            break;
                        }
                        if !send_daemon_message(&mut socket, message).await {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        tracing::warn!(%client_addr, skipped, "observation client lagged");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        observations = state.sessions.observe();
                    }
                }
            }
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
                    AttachedOutput::Structured(message) => send_daemon_message(&mut socket, *message).await,
                };
                if !sent {
                    break;
                }
            }
        }
    }

    for (_, attachment) in attachments {
        attachment.task.abort();
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
    api_token_authorized(state, token)
}

async fn handle_client_message(
    socket: &mut WebSocket,
    state: &AppState,
    client_addr: &str,
    output_tx: &mpsc::Sender<AttachedOutput>,
    attachments: &mut HashMap<SessionId, AttachedForwarder>,
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
    attachments: &mut HashMap<SessionId, AttachedForwarder>,
    action: WsAction,
) -> bool {
    match action {
        WsAction::Reply(message) => send_daemon_message(socket, message).await,
        WsAction::Resize {
            session_id,
            message,
        } => {
            if let Some(attachment) = attachments.get(&session_id) {
                attachment
                    .confirmation
                    .store(true, std::sync::atomic::Ordering::SeqCst);
            }
            send_daemon_message(socket, message).await
        }
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
                previous.task.abort();
            }
            let confirmation = attachment.confirmation.clone();
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
                                if output_tx.send(AttachedOutput::Structured(Box::new(message))).await.is_err() {
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
            attachments.insert(session_id, AttachedForwarder { task, confirmation });
            true
        }
        WsAction::Detach(session_id) => {
            if let Some(task) = attachments.remove(&session_id) {
                task.task.abort();
            }
            true
        }
    }
}

struct AttachedForwarder {
    task: JoinHandle<()>,
    confirmation: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

enum AttachedOutput {
    Binary(Vec<u8>),
    Structured(Box<DaemonMessage>),
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

fn current_time() -> protocol::Timestamp {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    protocol::Timestamp(i64::try_from(millis).unwrap_or(i64::MAX))
}
