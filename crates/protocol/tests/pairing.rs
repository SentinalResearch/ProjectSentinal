//! Milestone 4.E discovery and pairing contract (`/api/v1/discovery`).

#![allow(clippy::expect_used, clippy::unwrap_used)]

use protocol::pairing::DiscoveryResponse;
use serde_json::Value;
use std::path::PathBuf;

type Change = Box<dyn Fn(&mut DiscoveryResponse)>;

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
