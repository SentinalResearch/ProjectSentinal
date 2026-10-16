//! Milestone 4.E discovery and pairing contract (`/api/v1/discovery`).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use protocol::pairing::DiscoveryResponse;
use serde_json::Value;
use std::path::PathBuf;

type Change = Box<dyn Fn(&mut DiscoveryResponse)>;
type CreateChange = Box<dyn Fn(&mut protocol::pairing::PairingRequestCreate)>;

const ORIGIN: &str = "https://sentinal-pilot.example-tailnet.ts.net";

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pairing.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn discovery() -> DiscoveryResponse {
    serde_json::from_value(fixture()["discovery"].clone()).unwrap()
}

#[test]
fn discovery_round_trips_and_fits_its_size_bound() {
    let value = fixture()["discovery"].clone();
    let parsed = discovery();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    assert!(serde_json::to_vec(&parsed).unwrap().len() <= protocol::pairing::MAX_DISCOVERY_BYTES);
    assert!(parsed.validate(ORIGIN).is_ok());
}

#[test]
fn discovery_is_accepted_only_for_the_probed_origin_and_a_compatible_control() {
    let good = discovery();
    assert!(
        good.validate("https://other.example-tailnet.ts.net")
            .is_err(),
        "origin mismatch"
    );
    let cases: Vec<(&str, Change)> = vec![
        ("not a control", Box::new(|d| d.sentinal = "worker".into())),
        ("schema", Box::new(|d| d.schema_version = 2)),
        ("protocol", Box::new(|d| d.protocol.min = 2)),
        ("name", Box::new(|d| d.display_name = "x\u{1b}[31m".into())),
        ("long name", Box::new(|d| d.display_name = "n".repeat(65))),
        ("release", Box::new(|d| d.release = "0.1.18; rm".into())),
        ("id", Box::new(|d| d.control_machine_id.0 = "../x".into())),
    ];
    for (label, change) in cases {
        let mut bad = good.clone();
        change(&mut bad);
        assert!(bad.validate(ORIGIN).is_err(), "{label}");
    }
}

#[test]
fn unknown_fields_from_a_newer_control_are_tolerated() {
    let mut value = fixture()["discovery"].clone();
    value["future"] = Value::Bool(true);
    let parsed: DiscoveryResponse = serde_json::from_value(value).unwrap();
    assert!(parsed.validate(ORIGIN).is_ok());
}

fn round_trip<T: serde::de::DeserializeOwned + serde::Serialize>(key: &str) -> T {
    let value = fixture()[key].clone();
    let parsed: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), value, "{key}");
    parsed
}

#[test]
fn every_pairing_body_round_trips() {
    use protocol::pairing::*;
    let create: PairingRequestCreate = round_trip("request_create");
    assert!(create.validate().is_ok());
    round_trip::<PairingRequestCreated>("request_created");
    round_trip::<PairingProof>("proof");
    let list: PairingRequestList = round_trip("request_list");
    assert_eq!(list.requests[0].state, PairingRequestState::Pending);
    round_trip::<PairingApproval>("approval");
    round_trip::<PairingClaim>("claim");
    round_trip::<PairingClaimResult>("claim_pending");
    let done: PairingClaimResult = round_trip("claim_done");
    assert!(done.link.is_some());
    round_trip::<PairingCancel>("cancel");
}

#[test]
fn malformed_requests_are_refused_by_shape() {
    use protocol::pairing::PairingRequestCreate;
    let good: PairingRequestCreate =
        serde_json::from_value(fixture()["request_create"].clone()).unwrap();
    let cases: Vec<(&str, CreateChange)> = vec![
        (
            "id",
            Box::new(|r| r.machine.machine_id.0 = "not-a-ulid".into()),
        ),
        (
            "name",
            Box::new(|r| r.machine.display_name = "\u{202e}evil".into()),
        ),
        (
            "hash",
            Box::new(|r| r.request_secret_sha256 = "A".repeat(64)),
        ),
        (
            "short hash",
            Box::new(|r| r.request_secret_sha256 = "a".repeat(63)),
        ),
        ("nonce", Box::new(|r| r.worker_nonce = "zz".repeat(16))),
        (
            "version",
            Box::new(|r| r.machine.daemon_version = "9".repeat(33)),
        ),
    ];
    for (label, change) in cases {
        let mut bad = good.clone();
        change(&mut bad);
        assert!(bad.validate().is_err(), "{label}");
    }
    assert!(protocol::pairing::is_request_id("pr_9f2c1a0b7d3e4f51"));
    for bad in [
        "pr_9F2C1A0B7D3E4F51",
        "pc_9f2c1a0b7d3e4f51",
        "pr_../../etc",
        "pr_",
    ] {
        assert!(!protocol::pairing::is_request_id(bad), "{bad}");
    }
}

#[test]
fn verification_words_follow_the_shared_vector() {
    use protocol::pairing::{verification_transcript, verification_words};
    let vector = &fixture()["verification_vector"];
    let text = |key: &str| vector[key].as_str().unwrap().to_string();
    let transcript = verification_transcript(
        &protocol::MachineId(text("control_machine_id")),
        &text("request_id"),
        &text("control_nonce"),
        &text("request_secret_sha256"),
        &text("worker_nonce"),
    );
    assert!(transcript.starts_with(b"sentinal-pair-v1\x0001K2C7RQ"));
    assert_eq!(transcript.iter().filter(|byte| **byte == 0).count(), 5);
    let digest: Vec<u8> = (0..32)
        .map(|index| u8::from_str_radix(&text("sha256")[index * 2..index * 2 + 2], 16).unwrap())
        .collect();
    let expected: Vec<String> = vector["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|word| word.as_str().unwrap().to_string())
        .collect();
    assert_eq!(verification_words(&digest), expected);
    let ends = verification_words(&[0, 1, 255, 128]);
    assert_eq!(ends.len(), 4);
    assert_eq!(ends[0], "acorn");
    assert_eq!(ends[2], "zephyr");
}

#[test]
fn a_move_names_a_valid_control_only() {
    use protocol::{link::ControlUrlPolicy, pairing::LinkMoveRequest};
    let request: LinkMoveRequest = round_trip("move");
    assert!(request.validate(ControlUrlPolicy::Production).is_ok());
    for (url, id) in [
        ("http://100.64.0.1:7420", "01K2C7RQ9M3X8V6T4N2P0SDB5A"),
        ("https://evil.example.com", "01K2C7RQ9M3X8V6T4N2P0SDB5A"),
        ("https://c.example.ts.net", "not-a-ulid"),
    ] {
        let bad = LinkMoveRequest {
            control_url: url.into(),
            control_machine_id: protocol::MachineId(id.into()),
        };
        assert!(
            bad.validate(ControlUrlPolicy::Production).is_err(),
            "{url} {id}"
        );
    }
}
