//! Milestone 4.E control-release contract (`/api/v1/control/release`).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use protocol::release::{
    ControlReleaseView, ControlUpdateRequest, ControlUpdateStarted, ControlUpdateState,
    ReleaseChannel, ReleasePolicy, ReleasePolicyRequest,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::path::PathBuf;

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/release.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn round_trip<T: DeserializeOwned + Serialize>(value: &Value) -> T {
    let parsed: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), *value);
    parsed
}

#[test]
fn every_release_body_round_trips() {
    let fixture = fixture();
    let view: ControlReleaseView = round_trip(&fixture["view_available"]);
    assert_eq!(view.channel, ReleaseChannel::Stable);
    assert_eq!(view.policy, ReleasePolicy::Notify);
    let running: ControlReleaseView = round_trip(&fixture["view_running"]);
    assert_eq!(
        running.update.unwrap().state,
        ControlUpdateState::RolledBack
    );
    round_trip::<ControlUpdateRequest>(&fixture["update_request"]);
    round_trip::<ControlUpdateStarted>(&fixture["update_started"]);
    round_trip::<ReleasePolicyRequest>(&fixture["policy_request"]);
}

#[test]
fn update_requests_accept_only_strict_versions() {
    let valid = ControlUpdateRequest {
        version: "0.1.18".into(),
        allow_interrupt: false,
    };
    assert!(valid.validate().is_ok());
    for bad in ["0.1", "0.1.18.", "v0.1.18", "01.1.1", "0.1.18 ; rm", ""] {
        let request = ControlUpdateRequest {
            version: bad.into(),
            allow_interrupt: false,
        };
        assert!(request.validate().is_err(), "{bad}");
    }
}

#[test]
fn worker_update_bodies_round_trip_and_validate() {
    use protocol::worker_update::{WorkerUpdateRequest, WorkerUpdateState, WorkerUpdateStatus};
    let fixture = fixture();
    let request: WorkerUpdateRequest = round_trip(&fixture["worker_update_request"]);
    assert!(request.validate().is_ok());
    let status: WorkerUpdateStatus = round_trip(&fixture["worker_update_status"]);
    assert_eq!(status.state, WorkerUpdateState::Failed);
    for bad in ["0.1", "v0.1.18", "0.1.18;x", ""] {
        let request = WorkerUpdateRequest {
            version: bad.into(),
            allow_interrupt: true,
        };
        assert!(request.validate().is_err(), "{bad}");
    }
}
