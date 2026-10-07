//! Contract and validation tests for the additive Milestone 4.D HTTP bodies.

#![allow(clippy::unwrap_used)]

use protocol::{
    link::LinkHeartbeatResponse,
    notification::{
        NotificationFacts, NotificationIntent, NotificationIntentAcknowledgement,
        NotificationIntentBatch, PushProviderMode, PushProviderModeRequest, PushPublicKey,
        PushSubscriptionInfo, PushSubscriptionList, PushSubscriptionRequest,
    },
    MachineId, SessionId, Timestamp,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/native_push.json")).unwrap()
}

fn round_trip<T: DeserializeOwned + Serialize>(value: &Value) {
    let decoded: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), *value);
}

#[test]
fn native_push_http_contract_round_trips() {
    let value = fixture();
    round_trip::<NotificationIntentBatch>(&value["intent_batch"]);
    round_trip::<NotificationIntentAcknowledgement>(&value["intent_acknowledgement"]);
    round_trip::<PushPublicKey>(&value["public_key"]);
    round_trip::<PushProviderModeRequest>(&value["provider_mode_request"]);
    round_trip::<PushSubscriptionRequest>(&value["subscription_request"]);
    round_trip::<PushSubscriptionList>(&value["subscription_list"]);
}

#[test]
fn legacy_heartbeat_defaults_to_no_notification_ingress() {
    let heartbeat: LinkHeartbeatResponse = serde_json::from_value(serde_json::json!({
        "status": "ACTIVE",
        "display_name": "RTX PC",
        "next_heartbeat_ms": 30000,
        "refresh_due": false
    }))
    .unwrap();
    assert_eq!(heartbeat.notification_ingress, None);
    assert!(serde_json::to_value(heartbeat)
        .unwrap()
        .get("notification_ingress")
        .is_none());
}

#[test]
fn intent_validation_rejects_unsafe_or_oversized_fields() {
    let valid = NotificationIntent {
        intent_id: format!("ni_{}", "a".repeat(64)),
        event_id: protocol::EventId(8),
        session_id: SessionId(3),
        occurred_at: Timestamp(1_000),
        session_label: Some("Training".into()),
        facts: NotificationFacts::Waiting,
    };
    assert!(valid.validate().is_ok());

    let mut bad_id = valid.clone();
    bad_id.intent_id = "intent-8".into();
    assert!(bad_id.validate().is_err());

    let mut bad_event = valid.clone();
    bad_event.event_id = protocol::EventId(0);
    assert!(bad_event.validate().is_err());

    let mut bad_session = valid.clone();
    bad_session.session_id = SessionId(0);
    assert!(bad_session.validate().is_err());

    let mut bad_label = valid.clone();
    bad_label.session_label = Some("bad\nlabel".into());
    assert!(bad_label.validate().is_err());

    let mut long_label = valid;
    long_label.session_label = Some("x".repeat(193));
    assert!(long_label.validate().is_err());
}

#[test]
fn subscription_validation_enforces_exact_browser_key_shapes() {
    let value = fixture();
    let request: PushSubscriptionRequest =
        serde_json::from_value(value["subscription_request"].clone()).unwrap();
    assert!(request.validate().is_ok());

    let mut bad = request.clone();
    bad.endpoint = "http://push.example.invalid/path".into();
    assert!(bad.validate_shape().is_err());

    let mut bad = request.clone();
    bad.keys.auth = "not-base64!".into();
    assert!(bad.validate().is_err());

    let mut bad = request;
    bad.device_label = "x".repeat(193);
    assert!(bad.validate().is_err());
}

#[test]
fn secret_free_subscription_info_never_contains_endpoint_or_keys() {
    let info: PushSubscriptionInfo =
        serde_json::from_value(fixture()["subscription_list"]["devices"][0].clone()).unwrap();
    let debug = format!("{info:?}");
    let encoded = serde_json::to_string(&info).unwrap();
    for forbidden in ["endpoint", "p256dh", "auth"] {
        assert!(!debug.contains(forbidden));
        assert!(!encoded.contains(forbidden));
    }
}

#[test]
fn provider_mode_and_machine_binding_are_explicit() {
    let list: PushSubscriptionList =
        serde_json::from_value(fixture()["subscription_list"].clone()).unwrap();
    assert_eq!(list.provider_mode, PushProviderMode::WebPush);
    let batch: NotificationIntentBatch =
        serde_json::from_value(fixture()["intent_batch"].clone()).unwrap();
    assert_eq!(
        batch.machine_id,
        MachineId("01J9Z3K8V6Q2T7M4X5C0RNB8DA".into())
    );
}
