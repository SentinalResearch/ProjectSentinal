//! Owner-only `/api/v1/control/release` routes (4.E §11b).

use super::routes::{failure_response, reply, ControlRoutes};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use protocol::release::{ControlUpdateRequest, ReleasePolicyRequest};

/// Release routes; merged into the control router (shared state and body limit).
pub(super) fn routes() -> Router<ControlRoutes> {
    Router::new()
        .route("/api/v1/control/release", get(view))
        .route("/api/v1/control/release/check", post(check))
        .route("/api/v1/control/release/update", post(update))
        .route("/api/v1/control/release/policy", put(policy))
}

async fn view(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    reply(routes.owner_release(&headers).map(|release| release.view()))
}

async fn check(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    match routes.owner_release(&headers) {
        Ok(release) => reply(release.check().await),
        Err(failure) => failure_response(failure),
    }
}

async fn update(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Json(request): Json<ControlUpdateRequest>,
) -> Response {
    match routes
        .owner_release(&headers)
        .and_then(|release| release.start_update(&request))
    {
        Ok(started) => (StatusCode::ACCEPTED, Json(started)).into_response(),
        Err(failure) => failure_response(failure),
    }
}

async fn policy(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Json(request): Json<ReleasePolicyRequest>,
) -> Response {
    match routes.owner_release(&headers) {
        Ok(release) => reply(release.set_policy(request).await),
        Err(failure) => failure_response(failure),
    }
}
