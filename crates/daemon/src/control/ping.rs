//! Settings → Diagnostics: the control times its connection to one worker
//! and reports whether Tailscale reaches it directly or through a relay.

use super::proxy::{failure_response, owner_target, prove_identity, ProxyState};
use crate::{api::auth_routes::credential_from_headers, tailnet_dns};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{IntoResponse, Response},
    Json,
};
use protocol::link::{LinkEndpoint, MachinePing, TailnetPath};
use serde_json::Value;
use std::time::Instant;

/// Round trips timed per request; the first one may include connection setup.
const SAMPLES: u32 = 5;

pub(super) async fn machine_ping(
    State(state): State<ProxyState>,
    Path(machine_id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let supplied = credential_from_headers(&headers, state.development);
    let (target, _) = match owner_target(&state, supplied.as_deref(), machine_id) {
        Ok(value) => value,
        Err(error) => return failure_response(error),
    };
    let mut round_trips_us = Vec::new();
    let mut failed = 0;
    let mut error = None;
    for _ in 0..SAMPLES {
        let started = Instant::now();
        match prove_identity(&state, &target).await {
            Ok(()) => round_trips_us
                .push(u32::try_from(started.elapsed().as_micros()).unwrap_or(u32::MAX)),
            Err(failure) => {
                failed += 1;
                error = Some(failure.failure.message);
            }
        }
    }
    let status = tokio::task::spawn_blocking(tailnet_dns::real_status)
        .await
        .ok()
        .flatten();
    let (path, relay) = status
        .as_ref()
        .map_or((TailnetPath::Unknown, None), |status| {
            peer_path(status, &target.endpoint)
        });
    Json(MachinePing {
        machine_id: target.machine_id,
        round_trips_us,
        failed,
        error,
        path,
        relay,
    })
    .into_response()
}

/// How `tailscale status --json` says this Machine reaches `endpoint`:
/// `CurAddr` is set for a direct connection, `Relay` names the DERP region.
pub(super) fn peer_path(status: &Value, endpoint: &LinkEndpoint) -> (TailnetPath, Option<String>) {
    let dns_name = endpoint
        .dns_name
        .as_deref()
        .map(|name| name.trim_end_matches('.').to_ascii_lowercase());
    let peer = status
        .get("Peer")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|peers| peers.values())
        .find(|peer| {
            let by_address = peer
                .get("TailscaleIPs")
                .and_then(Value::as_array)
                .is_some_and(|ips| {
                    ips.iter()
                        .any(|ip| ip.as_str() == Some(endpoint.address.as_str()))
                });
            let by_name = peer
                .get("DNSName")
                .and_then(Value::as_str)
                .map(|name| name.trim_end_matches('.').to_ascii_lowercase())
                .is_some_and(|name| {
                    dns_name.as_deref() == Some(name.as_str())
                        || name == endpoint.address.to_ascii_lowercase()
                });
            by_address || by_name
        });
    let Some(peer) = peer else {
        return (TailnetPath::Unknown, None);
    };
    let text = |key: &str| {
        peer.get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    match (text("CurAddr"), text("Relay")) {
        (Some(_), _) => (TailnetPath::Direct, None),
        (None, Some(relay)) => (TailnetPath::Relayed, Some(relay)),
        (None, None) => (TailnetPath::Unknown, None),
    }
}

#[cfg(test)]
mod tests {
    use super::peer_path;
    use protocol::link::{LinkEndpoint, TailnetPath};
    use serde_json::json;

    fn endpoint(address: &str, dns_name: Option<&str>) -> LinkEndpoint {
        LinkEndpoint {
            address: address.into(),
            port: 7420,
            dns_name: dns_name.map(Into::into),
        }
    }

    fn status() -> serde_json::Value {
        json!({
            "Self": { "DNSName": "control.example.ts.net.", "TailscaleIPs": ["100.64.0.1"] },
            "Peer": {
                "a": { "DNSName": "direct.example.ts.net.", "TailscaleIPs": ["100.64.0.2"], "CurAddr": "203.0.113.5:41641", "Relay": "nyc" },
                "b": { "DNSName": "relayed.example.ts.net.", "TailscaleIPs": ["100.64.0.3"], "CurAddr": "", "Relay": "fra" },
                "c": { "DNSName": "idle.example.ts.net.", "TailscaleIPs": ["100.64.0.4"], "CurAddr": "", "Relay": "" }
            }
        })
    }

    #[test]
    fn a_current_address_is_direct_otherwise_the_relay_is_named() {
        let status = status();
        assert_eq!(
            peer_path(&status, &endpoint("100.64.0.2", None)),
            (TailnetPath::Direct, None)
        );
        assert_eq!(
            peer_path(&status, &endpoint("100.64.0.3", None)),
            (TailnetPath::Relayed, Some("fra".into()))
        );
        assert_eq!(
            peer_path(&status, &endpoint("100.64.0.4", None)),
            (TailnetPath::Unknown, None)
        );
    }

    #[test]
    fn peers_match_by_name_and_unknown_peers_say_unknown() {
        let status = status();
        assert_eq!(
            peer_path(&status, &endpoint("relayed.example.ts.net", None)),
            (TailnetPath::Relayed, Some("fra".into()))
        );
        assert_eq!(
            peer_path(
                &status,
                &endpoint("192.0.2.9", Some("direct.example.ts.net."))
            ),
            (TailnetPath::Direct, None)
        );
        assert_eq!(
            peer_path(&status, &endpoint("127.0.0.1", None)),
            (TailnetPath::Unknown, None)
        );
    }
}
