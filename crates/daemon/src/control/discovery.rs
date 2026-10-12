//! `GET /api/v1/discovery` (4.E §5): an unauthenticated, control-only answer
//! naming this control for Tailnet peers. The daemon binds loopback, so only
//! Tailscale Serve reaches it. The rate limit is control-wide because the
//! `Tailscale-User-Login` header is spoofable from loopback (F10).

use super::{routes::ControlRoutes, ControlService};
use axum::{
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use protocol::pairing::{
    DiscoveryResponse, PairingSupport, ProtocolRange, DISCOVERY_SCHEMA_VERSION,
};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

/// Discovery answers per minute, across all callers.
pub const DISCOVERY_PER_MINUTE: u32 = 60;

/// A fixed one-minute window.
#[derive(Debug)]
pub(crate) struct DiscoveryLimit(Mutex<(Instant, u32)>);

impl Default for DiscoveryLimit {
    fn default() -> Self {
        Self(Mutex::new((Instant::now(), 0)))
    }
}

impl DiscoveryLimit {
    fn admit(&self) -> bool {
        let Ok(mut window) = self.0.lock() else {
            return false;
        };
        if window.0.elapsed() >= Duration::from_secs(60) {
            *window = (Instant::now(), 0);
        }
        window.1 += 1;
        window.1 <= DISCOVERY_PER_MINUTE
    }
}

impl ControlService {
    /// What this control tells Tailnet peers about itself.
    pub fn discovery(&self) -> DiscoveryResponse {
        let settings = &self.settings;
        DiscoveryResponse {
            sentinal: "control".into(),
            schema_version: DISCOVERY_SCHEMA_VERSION,
            control_machine_id: settings.machine_id.clone(),
            display_name: settings
                .discovery_name
                .clone()
                .unwrap_or_else(|| settings.display_name.clone()),
            origin: settings.public_url.trim_end_matches('/').to_string(),
            release: settings.daemon_version.clone(),
            protocol: ProtocolRange {
                min: protocol::PROTOCOL_VERSION,
                max: protocol::PROTOCOL_VERSION,
            },
            pairing: PairingSupport {
                approval: false,
                code: true,
            },
        }
    }
}

/// The discovery route; merged into the control router.
pub(super) fn routes() -> Router<ControlRoutes> {
    Router::new().route("/api/v1/discovery", get(discovery))
}

async fn discovery(State(routes): State<ControlRoutes>) -> Response {
    let Ok(control) = routes.control() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    if !control.discovery_limit.admit() {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let mut response = Json(control.discovery()).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
