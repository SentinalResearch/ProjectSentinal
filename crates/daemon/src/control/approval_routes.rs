//! `/api/v1/pairing/requests` (4.E §6). Creating, claiming, and cancelling
//! are unauthenticated (the request secret and the callback proof carry the
//! authority); listing and deciding need an owner browser Session.

use super::routes::{failure_response, reply, ControlRoutes};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
    Json, Router,
};
use protocol::pairing::{
    PairingApproval, PairingCancel, PairingClaim, PairingRequestCreate, PairingRequestState,
};

/// Approval routes; merged into the control router (shared body limit).
pub(super) fn routes() -> Router<ControlRoutes> {
    Router::new()
        .route("/api/v1/pairing/requests", post(create).get(list))
        .route(
            "/api/v1/pairing/requests/{request_id}",
            axum::routing::delete(cancel),
        )
        .route(
            "/api/v1/pairing/requests/{request_id}/approve",
            post(approve),
        )
        .route("/api/v1/pairing/requests/{request_id}/reject", post(reject))
        .route("/api/v1/pairing/requests/{request_id}/claim", post(claim))
}

async fn create(
    State(routes): State<ControlRoutes>,
    Json(request): Json<PairingRequestCreate>,
) -> Response {
    let control = match routes.control() {
        Ok(control) => control.clone(),
        Err(failure) => return failure_response(failure),
    };
    match control.create_pairing_request(request).await {
        Ok(created) => (StatusCode::CREATED, Json(created)).into_response(),
        Err(failure) => failure_response(failure),
    }
}

async fn list(State(routes): State<ControlRoutes>, headers: HeaderMap) -> Response {
    reply(
        routes
            .owner(&headers)
            .and_then(|control| control.pairing_requests()),
    )
}

#[derive(serde::Serialize)]
struct Decision {
    state: PairingRequestState,
}

async fn approve(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
    Json(approval): Json<PairingApproval>,
) -> Response {
    reply(
        routes
            .owner(&headers)
            .and_then(|control| control.approve_pairing_request(&request_id, &approval))
            .map(|state| Decision { state }),
    )
}

async fn reject(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Path(request_id): Path<String>,
) -> Response {
    reply(
        routes
            .owner(&headers)
            .and_then(|control| control.reject_pairing_request(&request_id))
            .map(|state| Decision { state }),
    )
}

async fn claim(
    State(routes): State<ControlRoutes>,
    Path(request_id): Path<String>,
    Json(claim): Json<PairingClaim>,
) -> Response {
    let result = routes
        .control()
        .and_then(|control| control.claim_pairing_request(&request_id, &claim));
    match result {
        Ok(result) => {
            let status = match result.state {
                PairingRequestState::Claimed if result.link.is_some() => StatusCode::OK,
                // Already claimed: the credential is issued exactly once.
                PairingRequestState::Claimed => StatusCode::CONFLICT,
                PairingRequestState::Pending | PairingRequestState::Approved => {
                    StatusCode::ACCEPTED
                }
                PairingRequestState::Rejected => StatusCode::FORBIDDEN,
                PairingRequestState::Expired | PairingRequestState::Cancelled => StatusCode::GONE,
            };
            let mut response = (status, Json(result)).into_response();
            response.headers_mut().insert(
                axum::http::header::CACHE_CONTROL,
                axum::http::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(failure) => failure_response(failure),
    }
}

async fn cancel(
    State(routes): State<ControlRoutes>,
    Path(request_id): Path<String>,
    Json(cancel): Json<PairingCancel>,
) -> Response {
    match routes
        .control()
        .and_then(|control| control.cancel_pairing_request(&request_id, &cancel))
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(failure) => failure_response(failure),
    }
}
