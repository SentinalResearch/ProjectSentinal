//! Owner-only health routes (milestone 4.H).

use super::routes::{failure_response, reply, ControlRoutes};
use super::timing::ServerTiming;
use axum::{
    body::Bytes,
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue},
    response::Response,
    routing::{get, post},
    Router,
};
use protocol::{
    health_report::{HealthRange, ReportNames},
    link::LinkFailureCode,
    observability::HealthSampleBatch,
};

#[derive(serde::Deserialize)]
struct RangeQuery {
    range: Option<HealthRange>,
    names: Option<ReportNames>,
}

impl RangeQuery {
    fn range(&self) -> HealthRange {
        self.range.unwrap_or(HealthRange::Day)
    }
}

/// Health routes; merged into the control router (shared state and the
/// 64 KiB body limit).
pub(super) fn routes() -> Router<ControlRoutes> {
    Router::new()
        .route("/api/v1/health/samples", post(samples))
        .route("/api/v1/health/summary", get(summary))
        .route("/api/v1/health/incidents", get(incidents))
        .route("/api/v1/health/report", get(report))
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

fn private(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-store, private"),
    );
    response
}

async fn summary(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Query(query): Query<RangeQuery>,
) -> Response {
    let mut timing = ServerTiming::default();
    match timing.measure("auth", || routes.owner(&headers)) {
        Ok(control) => {
            let response =
                timing.measure("summary", || reply(control.health_summary(query.range())));
            private(timing.apply(response))
        }
        Err(failure) => failure_response(failure),
    }
}

async fn incidents(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Query(query): Query<RangeQuery>,
) -> Response {
    let mut timing = ServerTiming::default();
    match timing.measure("auth", || routes.owner(&headers)) {
        Ok(control) => {
            let response = timing.measure("incidents", || {
                reply(control.health_incident_list(query.range()))
            });
            private(timing.apply(response))
        }
        Err(failure) => failure_response(failure),
    }
}

async fn report(
    State(routes): State<ControlRoutes>,
    headers: HeaderMap,
    Query(query): Query<RangeQuery>,
) -> Response {
    let mut timing = ServerTiming::default();
    let range = query.range();
    let names = query.names.unwrap_or(ReportNames::Anonymous);
    match timing.measure("auth", || routes.owner(&headers)) {
        Ok(control) => {
            let mut response =
                timing.measure("report", || reply(control.health_report(range, names)));
            if response.status().is_success() {
                let file = match range {
                    HealthRange::Hour => "1h",
                    HealthRange::Day => "24h",
                    HealthRange::Week => "7d",
                    HealthRange::Month => "30d",
                };
                if let Ok(value) = HeaderValue::from_str(&format!(
                    "attachment; filename=\"sentinal-health-{file}.json\""
                )) {
                    response
                        .headers_mut()
                        .insert(header::CONTENT_DISPOSITION, value);
                }
            }
            private(timing.apply(response))
        }
        Err(failure) => failure_response(failure),
    }
}
