//! HTTP routes for the control role: owner registry/pairing and worker links.
//!
//! Owner routes accept only an owner **browser Session**; the Machine token is
//! refused so a local script cannot mint pairing codes. Worker routes take a
//! one-time code (exchange) or a worker credential (heartbeat, refresh,
//! disconnect). Secrets travel only in bodies and Authorization headers.

use super::{proxy, ControlFailure, ControlService};
use crate::{
    api::auth_routes::{bearer_from_headers, credential_from_headers},
    auth::{AuthManager, CredentialKind},
    push::gateway::{PushGateway, PushGatewayError},
};
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use protocol::{
    link::{
        LinkDisconnectRequest, LinkExchangeRequest, LinkFailure, LinkFailureCode,
        LinkHeartbeatRequest, LinkRefreshRequest, RenameMachineRequest,
    },
    MachineId, NotificationIntentBatch, PushProviderModeRequest, PushSubscriptionRequest,
};
use std::net::SocketAddr;

/// Link bodies are small; anything larger is refused before parsing.
const MAX_LINK_BODY_BYTES: usize = 64 * 1024;

/// State for the control routes.
#[derive(Clone)]
pub struct ControlRoutes {
    control: Option<ControlService>,
    auth: AuthManager,
    behind_tailscale_serve: bool,
    development: bool,
    push: Option<PushGateway>,
}

impl ControlRoutes {
    /// Routes for a daemon; `control` is `None` unless the control role is enabled.
    pub fn new(
        control: Option<ControlService>,
        auth: AuthManager,
        behind_tailscale_serve: bool,
        development: bool,
    ) -> Self {
        ControlRoutes {
            control,
            auth,
            behind_tailscale_serve,
            development,
            push: None,
        }
    }

    /// Attach the optional control-owned native Push gateway.
    pub fn with_push(mut self, push: Option<PushGateway>) -> Self {
        self.push = push;
        self
    }

    /// Build the router.
    pub fn router(self) -> Router {
        let proxy = proxy::routes(self.control.clone(), self.auth.clone(), self.development);
        Router::new()
            .route("/api/v1/pairing/codes", post(create_code))
            .route(
                "/api/v1/pairing/codes/{code_id}",
                get(code_status).delete(cancel_code),
            )
            .route("/api/v1/machines", get(registry))
            .route(
                "/api/v1/machines/{machine_id}",
                axum::routing::patch(rename).delete(revoke),
            )
            .route("/api/v1/link/exchange", post(exchange))
            .route("/api/v1/link/heartbeat", post(heartbeat))
            .route("/api/v1/link/refresh", post(refresh))
            .route("/api/v1/link/disconnect", post(disconnect))
            .route(
                "/api/v1/link/notification-intents",
                post(notification_intents),
            )
            .route("/api/v1/push/public-key", get(push_public_key))
            .route("/api/v1/push/deliveries", get(push_deliveries))
            .route(
                "/api/v1/push/subscriptions",
                get(push_subscriptions).post(push_subscribe),
            )
            .route("/api/v1/push/subscriptions/refresh", post(push_subscribe))
            .route(
                "/api/v1/push/subscriptions/{subscription_id}",
                axum::routing::delete(push_unsubscribe),
            )
            .route("/api/v1/push/provider", put(push_provider))
            .layer(DefaultBodyLimit::max(MAX_LINK_BODY_BYTES))
            .with_state(self)
            .merge(proxy)
    }

    fn control(&self) -> Result<&ControlService, ControlFailure> {
        self.control.as_ref().ok_or_else(|| {
            ControlFailure::new(
                404,
                LinkFailureCode::NotControl,
                "this daemon is not a control Machine",
            )
        })
    }

    /// Require an owner browser Session (never the Machine token).
    fn owner(&self, headers: &HeaderMap) -> Result<&ControlService, ControlFailure> {
        let control = self.control()?;
        let kind = credential_from_headers(headers, self.development)
            .and_then(|token| self.auth.authorize(&token).ok().flatten());
        match kind {
            Some(CredentialKind::BrowserSession) => Ok(control),
            Some(CredentialKind::MachineToken) => Err(ControlFailure::new(
                403,
                LinkFailureCode::Unauthorized,
                "Machine registry changes need an owner browser Session",
            )),
            None => Err(ControlFailure::new(
                401,
                LinkFailureCode::Unauthorized,
                "sign in as the owner first",
            )),
        }
    }

    fn owner_push(&self, headers: &HeaderMap) -> Result<&PushGateway, ControlFailure> {
        self.owner(headers)?;
        self.push.as_ref().ok_or_else(|| {
            ControlFailure::new(
                404,
                LinkFailureCode::NotControl,
                "native Push is available only on the control Machine",
            )
        })
    }

    /// Tailscale user when published by Tailscale Serve, else the peer address.
    fn peer(&self, headers: &HeaderMap, address: SocketAddr) -> String {
        if self.behind_tailscale_serve && address.ip().is_loopback() {
            if let Some(login) = headers
                .get("tailscale-user-login")
                .and_then(|value| value.to_str().ok())
                .filter(|login| !login.is_empty() && login.len() <= 256)
            {
                return login.to_string();
            }
        }
        address.ip().to_string()
    }
}

async fn create_code(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    match routes.owner(&headers) {
        Ok(control) => reply(control.create_code()),
        Err(failure) => failure_response(failure),
    }
}

async fn code_status(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Path(code_id): Path<String>,
) -> Response {
    match routes.owner(&headers) {
        Ok(control) => reply(control.code_status(&code_id)),
        Err(failure) => failure_response(failure),
    }
}

async fn cancel_code(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Path(code_id): Path<String>,
) -> Response {
    match routes.owner(&headers) {
        Ok(control) => no_content(control.cancel_code(&code_id)),
        Err(failure) => failure_response(failure),
    }
}

async fn registry(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    match routes.owner(&headers) {
        Ok(control) => reply(control.registry()),
        Err(failure) => failure_response(failure),
    }
}

async fn rename(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Path(machine_id): Path<String>,
    Json(request): Json<RenameMachineRequest>,
) -> Response {
    match routes.owner(&headers) {
        Ok(control) => reply(control.rename(&MachineId(machine_id), &request.display_name)),
        Err(failure) => failure_response(failure),
    }
}

async fn revoke(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Path(machine_id): Path<String>,
) -> Response {
    match routes.owner(&headers) {
        Ok(control) => no_content(control.revoke(&MachineId(machine_id))),
        Err(failure) => failure_response(failure),
    }
}

async fn exchange(
    State(routes): State<ControlRoutes>,
    ConnectInfo(address): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<LinkExchangeRequest>,
) -> Response {
    let peer = routes.peer(&headers, address);
    match routes.control() {
        Ok(control) => reply(control.exchange(request, &peer)),
        Err(failure) => failure_response(failure),
    }
}

async fn heartbeat(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Json(request): Json<LinkHeartbeatRequest>,
) -> Response {
    match routes.control() {
        Ok(control) => {
            reply(control.heartbeat(bearer_from_headers(&headers).unwrap_or_default(), request))
        }
        Err(failure) => failure_response(failure),
    }
}

async fn notification_intents(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Json(request): Json<NotificationIntentBatch>,
) -> Response {
    match routes.control() {
        Ok(control) => reply(control.accept_notification_intents(
            bearer_from_headers(&headers).unwrap_or_default(),
            request,
        )),
        Err(failure) => failure_response(failure),
    }
}

async fn push_public_key(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    match routes.owner_push(&headers) {
        Ok(push) => push_reply(push.public_key()),
        Err(failure) => failure_response(failure),
    }
}

async fn push_subscriptions(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    match routes.owner_push(&headers) {
        Ok(push) => push_reply(push.subscriptions()),
        Err(failure) => failure_response(failure),
    }
}

async fn push_deliveries(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    match routes.owner_push(&headers) {
        Ok(push) => push_reply(push.delivery_history()),
        Err(failure) => failure_response(failure),
    }
}

async fn push_subscribe(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Json(request): Json<PushSubscriptionRequest>,
) -> Response {
    match routes.owner_push(&headers) {
        Ok(push) => push_reply(push.subscribe(request).await),
        Err(failure) => failure_response(failure),
    }
}

async fn push_unsubscribe(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Path(subscription_id): Path<String>,
) -> Response {
    match routes.owner_push(&headers) {
        Ok(push) => match push.unsubscribe(&subscription_id) {
            Ok(()) => StatusCode::NO_CONTENT.into_response(),
            Err(error) => push_error(error),
        },
        Err(failure) => failure_response(failure),
    }
}

async fn push_provider(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Json(request): Json<PushProviderModeRequest>,
) -> Response {
    match routes.owner_push(&headers) {
        Ok(push) => push_reply(push.set_provider_mode(request.mode)),
        Err(failure) => failure_response(failure),
    }
}

fn push_reply<T: serde::Serialize>(result: Result<T, PushGatewayError>) -> Response {
    match result {
        Ok(value) => Json(value).into_response(),
        Err(error) => push_error(error),
    }
}

fn push_error(error: PushGatewayError) -> Response {
    let (status, code) = match error {
        PushGatewayError::Invalid(_) | PushGatewayError::Endpoint(_) => {
            (422, LinkFailureCode::InvalidRequest)
        }
        PushGatewayError::NotFound(_) => (404, LinkFailureCode::NotFound),
        PushGatewayError::NoDevices => (409, LinkFailureCode::InvalidRequest),
        PushGatewayError::Internal => (500, LinkFailureCode::Internal),
    };
    failure_response(ControlFailure::new(status, code, error.to_string()))
}

async fn refresh(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Json(request): Json<LinkRefreshRequest>,
) -> Response {
    match routes.control() {
        Ok(control) => {
            reply(control.refresh(bearer_from_headers(&headers).unwrap_or_default(), request))
        }
        Err(failure) => failure_response(failure),
    }
}

async fn disconnect(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Json(request): Json<LinkDisconnectRequest>,
) -> Response {
    match routes.control() {
        Ok(control) => no_content(control.disconnect(
            bearer_from_headers(&headers).unwrap_or_default(),
            &request.machine_id,
        )),
        Err(failure) => failure_response(failure),
    }
}

fn reply<T: serde::Serialize>(result: Result<T, ControlFailure>) -> Response {
    match result {
        Ok(body) => Json(body).into_response(),
        Err(failure) => failure_response(failure),
    }
}

fn no_content(result: Result<(), ControlFailure>) -> Response {
    match result {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(failure) => failure_response(failure),
    }
}

fn failure_response(failure: ControlFailure) -> Response {
    let status = StatusCode::from_u16(failure.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json::<LinkFailure>(failure.failure)).into_response()
}
