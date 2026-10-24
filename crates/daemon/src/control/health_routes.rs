//! Owner-only health routes (milestone 4.H).

use super::routes::{failure_response, reply, ControlRoutes};
use axum::{
    body::Bytes, extract::State, http::HeaderMap, response::Response, routing::post, Router,
};
use protocol::{link::LinkFailureCode, observability::HealthSampleBatch};

/// Health routes; merged into the control router (shared state and the
/// 64 KiB body limit).
pub(super) fn routes() -> Router<ControlRoutes> {
    Router::new().route("/api/v1/health/samples", post(samples))
}

/// Browser samples. Takes raw bytes so `navigator.sendBeacon` (any content
/// type) works the same as `fetch`.
async fn samples(State(routes): State<ControlRoutes>, headers: HeaderMap, body: Bytes) -> Response {
    let control = match routes.owner(&headers) {
        Ok(control) => control,
        Err(failure) => return failure_response(failure),
    };
    match serde_json::from_slice::<HealthSampleBatch>(&body) {
        Ok(batch) => reply(control.accept_browser_samples(&batch)),
        Err(_) => failure_response(super::ControlFailure::new(
            400,
            LinkFailureCode::InvalidRequest,
            "not a health sample batch",
        )),
    }
}
