//! Milestone 6.0 agent tokens and levels.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use protocol::agent::{
    validate_agent_name, AgentLevel, AgentList, AgentScreen, CreateAgentRequest, CreatedAgent,
    OutputGrants, ProposeEventsRequest, ProposeEventsResponse,
};
use serde_json::{json, Value};
use std::path::PathBuf;

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/agent.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn agent_messages_round_trip() {
    let value = fixture();
    let create: CreateAgentRequest = serde_json::from_value(value["create"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&create).unwrap(), value["create"]);
    let created: CreatedAgent = serde_json::from_value(value["created"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&created).unwrap(), value["created"]);
    let list: AgentList = serde_json::from_value(value["list"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&list).unwrap(), value["list"]);
    let screen: AgentScreen = serde_json::from_value(value["screen"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&screen).unwrap(), value["screen"]);
    let propose: ProposeEventsRequest = serde_json::from_value(value["propose"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&propose).unwrap(), value["propose"]);
    let proposed: ProposeEventsResponse = serde_json::from_value(value["proposed"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&proposed).unwrap(), value["proposed"]);
    let grants: OutputGrants = serde_json::from_value(value["grants"].clone()).unwrap();
    assert_eq!(serde_json::to_value(&grants).unwrap(), value["grants"]);
}

#[test]
fn output_access_is_off_unless_asked_for() {
    let request: CreateAgentRequest = serde_json::from_value(json!({ "name": "x", "level": "OBSERVE" })).unwrap();
    assert!(!request.output_access);
}

#[test]
fn the_token_never_shows_in_debug_output() {
    let created: CreatedAgent = serde_json::from_value(fixture()["created"].clone()).unwrap();
    assert!(!format!("{created:?}").contains("0123456789abcdef"));
}

#[test]
fn levels_are_ordered_and_level_three_does_not_exist() {
    assert!(AgentLevel::Suggest.allows(AgentLevel::Observe));
    assert!(AgentLevel::Annotate.allows(AgentLevel::Annotate));
    assert!(!AgentLevel::Observe.allows(AgentLevel::Annotate));
    assert_eq!(
        [
            AgentLevel::Observe,
            AgentLevel::Annotate,
            AgentLevel::Suggest
        ]
        .map(AgentLevel::number),
        [0, 1, 2]
    );
    for name in ["ACT", "ACT_WITH_APPROVAL", "ACT_ALONE", "ADMIN", "3"] {
        let request = json!({ "name": "x", "level": name });
        assert!(
            serde_json::from_value::<CreateAgentRequest>(request).is_err(),
            "{name}"
        );
    }
}

#[test]
fn agent_names_are_short_and_plain() {
    for good in ["claude", "codex-ci", "aider.local", "a", "x_1"] {
        assert!(validate_agent_name(good).is_ok(), "{good}");
    }
    for bad in [
        "",
        "-lead",
        ".hidden",
        "has space",
        "claude\u{1b}[31m",
        "ünïcode",
        &"n".repeat(33),
        "a/b",
    ] {
        assert!(validate_agent_name(bad).is_err(), "{bad:?}");
    }
}
