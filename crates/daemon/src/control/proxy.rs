//! Owner-authenticated control-origin proxy to one enrolled worker.

use super::{ControlFailure, ControlService, RoutingTarget};
use crate::{
    api::auth_routes::credential_from_headers,
    auth::{AuthManager, CredentialKind},
};
use axum::{
    body::Body,
    extract::{
        ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade},
        Path, RawQuery, State,
    },
    http::{HeaderMap, HeaderName, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use protocol::{
    link::{LinkFailureCode, LinkIdentity},
    MachineId,
};
use std::time::Duration;
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::{header::AUTHORIZATION, HeaderValue},
    Message as UpstreamMessage,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub(super) struct ProxyState {
    pub(super) control: Option<ControlService>,
    pub(super) auth: AuthManager,
    pub(super) http: reqwest::Client,
    pub(super) development: bool,
}

pub(super) fn routes(
    control: Option<ControlService>,
    auth: AuthManager,
    development: bool,
) -> Router {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    Router::new()
        .route("/api/v1/machines/{machine_id}/ws", any(routed_websocket))
        .route(
            "/api/v1/machines/{machine_id}/sessions/{session_id}/output",
            get(routed_output),
        )
        .route(
            "/api/v1/machines/{machine_id}/ping",
            get(super::ping::machine_ping),
        )
        .with_state(ProxyState {
            control,
            auth,
            http,
            development,
        })
}

pub(super) fn owner_target(
    state: &ProxyState,
    token: Option<&str>,
    machine_id: String,
) -> Result<(RoutingTarget, String), ControlFailure> {
    let token = token.ok_or_else(unauthorized)?;
    match state.auth.authorize(token) {
        Ok(Some(CredentialKind::BrowserSession)) => {}
        Ok(_) => return Err(unauthorized()),
        Err(error) => {
            tracing::error!(%error, "routing authentication failed");
            return Err(ControlFailure::new(
                500,
                LinkFailureCode::Internal,
                "the control could not verify the owner Session",
            ));
        }
    }
    let control = state.control.as_ref().ok_or_else(|| {
        ControlFailure::new(
            404,
            LinkFailureCode::NotControl,
            "this daemon is not a control Machine",
        )
    })?;
    let target = control.routing_target(&MachineId(machine_id))?;
    Ok((target, token.to_string()))
}

pub(super) async fn prove_identity(
    state: &ProxyState,
    target: &RoutingTarget,
) -> Result<(), ControlFailure> {
    verify_identity(&state.http, target).await
}

/// One authenticated `/link/identity` round trip that must return `target`'s ID.
pub(super) async fn verify_identity(
    http: &reqwest::Client,
    target: &RoutingTarget,
) -> Result<(), ControlFailure> {
    let response = http
        .get(worker_url(target, "http", "/api/v1/link/identity"))
        .bearer_auth(target.control_credential.expose())
        .send()
        .await
        .map_err(unreachable_worker)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(ControlFailure::new(
            502,
            LinkFailureCode::Unauthorized,
            "the worker refused the control credential",
        ));
    }
    let identity = response
        .json::<LinkIdentity>()
        .await
        .map_err(unreachable_worker)?;
    if identity.machine_id != target.machine_id {
        return Err(ControlFailure::new(
            502,
            LinkFailureCode::InvalidMachine,
            "the worker endpoint proved a different Machine identity",
        ));
    }
    Ok(())
}

async fn routed_websocket(
    State(state): State<ProxyState>,
    Path(machine_id): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    let supplied = credential_from_headers(&headers, state.development).or_else(|| {
        state
            .development
            .then(|| query.as_deref().and_then(token_from_query))
            .flatten()
            .map(str::to_string)
    });
    let (target, owner_token) = match owner_target(&state, supplied.as_deref(), machine_id) {
        Ok(value) => value,
        Err(error) => return failure_response(error),
    };
    // Identity proof + upstream connect: the terminal-open cost Link v2 must beat.
    let setup_started = std::time::Instant::now();
    if let Err(error) = prove_identity(&state, &target).await {
        return failure_response(error);
    }
    let mut request = match worker_url(&target, "ws", "/api/v1/ws").into_client_request() {
        Ok(request) => request,
        Err(error) => return failure_response(unreachable_worker(error)),
    };
    let bearer =
        match HeaderValue::from_str(&format!("Bearer {}", target.control_credential.expose())) {
            Ok(value) => value,
            Err(error) => return failure_response(unreachable_worker(error)),
        };
    request.headers_mut().insert(AUTHORIZATION, bearer);
    let upstream =
        match tokio::time::timeout(CONNECT_TIMEOUT, crate::nodelay::connect_websocket(request))
            .await
        {
            Ok(Ok((socket, _))) => socket,
            Ok(Err(error)) => return failure_response(unreachable_worker(error)),
            Err(error) => return failure_response(unreachable_worker(error)),
        };
    if let Some(control) = &state.control {
        control.record_route(
            &target.machine_id,
            protocol::observability::HealthLabel::RouteTerminalSetup,
            setup_started.elapsed(),
        );
    }
    upgrade
        .on_upgrade(move |browser| proxy_socket(browser, upstream, state.auth, owner_token))
        .into_response()
}

async fn proxy_socket(
    mut browser: WebSocket,
    mut upstream: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    auth: AuthManager,
    owner_token: String,
) {
    loop {
        if !matches!(
            auth.authorize(&owner_token),
            Ok(Some(CredentialKind::BrowserSession))
        ) {
            let _ = browser
                .send(Message::Close(Some(CloseFrame {
                    code: 4_001,
                    reason: "browser session expired".into(),
                })))
                .await;
            break;
        }
        tokio::select! {
            from_browser = browser.recv() => {
                let Some(message) = from_browser else { break; };
                let Ok(message) = message else { break; };
                if upstream.send(to_upstream(message)).await.is_err() { break; }
            }
            from_worker = upstream.next() => {
                let Some(message) = from_worker else { break; };
                let Ok(message) = message else { break; };
                if browser.send(to_browser(message)).await.is_err() { break; }
            }
        }
    }
    let _ = upstream.close(None).await;
}

async fn routed_output(
    State(state): State<ProxyState>,
    Path((machine_id, session_id)): Path<(String, u64)>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let supplied = credential_from_headers(&headers, state.development);
    let (target, _) = match owner_target(&state, supplied.as_deref(), machine_id) {
        Ok(value) => value,
        Err(error) => return failure_response(error),
    };
    if let Err(error) = prove_identity(&state, &target).await {
        return failure_response(error);
    }
    let mut path = format!("/api/v1/sessions/{session_id}/output");
    if let Some(query) = query {
        path.push('?');
        path.push_str(&query);
    }
    let response = match state
        .http
        .get(worker_url(&target, "http", &path))
        .bearer_auth(target.control_credential.expose())
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => return failure_response(unreachable_worker(error)),
    };
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => return failure_response(unreachable_worker(error)),
    };
    let mut reply = Response::builder().status(status);
    for name in [
        "content-type",
        "x-sentinal-output-warning",
        "x-sentinal-output-complete",
        "x-sentinal-output-next-offset",
    ] {
        if let (Ok(name), Some(value)) = (HeaderName::try_from(name), headers.get(name)) {
            reply = reply.header(name, value);
        }
    }
    reply
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

pub(super) fn worker_url(target: &RoutingTarget, scheme: &str, path: &str) -> String {
    let address = if target.endpoint.address.contains(':') {
        format!("[{}]", target.endpoint.address)
    } else {
        target.endpoint.address.clone()
    };
    format!("{scheme}://{address}:{}{path}", target.endpoint.port)
}

fn to_upstream(message: Message) -> UpstreamMessage {
    match message {
        Message::Text(value) => UpstreamMessage::Text(value.as_str().into()),
        Message::Binary(value) => UpstreamMessage::Binary(value),
        Message::Ping(value) => UpstreamMessage::Ping(value),
        Message::Pong(value) => UpstreamMessage::Pong(value),
        Message::Close(_) => UpstreamMessage::Close(None),
    }
}

fn to_browser(message: UpstreamMessage) -> Message {
    match message {
        UpstreamMessage::Text(value) => Message::Text(value.as_str().into()),
        UpstreamMessage::Binary(value) => Message::Binary(value),
        UpstreamMessage::Ping(value) => Message::Ping(value),
        UpstreamMessage::Pong(value) => Message::Pong(value),
        UpstreamMessage::Close(_) => Message::Close(None),
        UpstreamMessage::Frame(_) => Message::Close(None),
    }
}

fn unauthorized() -> ControlFailure {
    ControlFailure::new(
        401,
        LinkFailureCode::Unauthorized,
        "sign in as the owner first",
    )
}

fn token_from_query(query: &str) -> Option<&str> {
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix("token="))
}

fn unreachable_worker(error: impl std::fmt::Display) -> ControlFailure {
    tracing::warn!(%error, "worker routing failed");
    ControlFailure::new(
        502,
        LinkFailureCode::Internal,
        "the selected Machine is unreachable",
    )
}

pub(super) fn failure_response(failure: ControlFailure) -> Response {
    let status = StatusCode::from_u16(failure.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, axum::Json(failure.failure)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{link::MachineCapability, Secret};

    #[test]
    fn worker_urls_bracket_ipv6() {
        let target = RoutingTarget {
            machine_id: MachineId("worker".into()),
            endpoint: protocol::link::LinkEndpoint {
                address: "fd7a:115c:a1e0::1".into(),
                port: 7420,
                dns_name: None,
            },
            control_credential: Secret::new("a".repeat(64)),
            capabilities: vec![MachineCapability::Sessions],
        };
        assert_eq!(
            worker_url(&target, "ws", "/api/v1/ws"),
            "ws://[fd7a:115c:a1e0::1]:7420/api/v1/ws"
        );
    }
}
