//! Contract tests: freeze the JSON wire format.
//!
//! Each fixture in `tests/fixtures/` is a real example message. For each one we
//! check that:
//!   1. it parses into the Rust type (old messages keep working), and
//!   2. serialising it again gives the same JSON (no field renamed or dropped).
//!
//! If one of these fails, you changed the protocol. Read `docs/TESTING.md`.

// Test helpers may panic freely; a panic is a test failure.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use protocol::{
    AuthChangePasswordRequest, AuthFailure, AuthLoginRequest, AuthSession, AuthSessionInfo,
    AuthSetupRequest, AuthStatus, ClientRequest, Command, DaemonMessage, Envelope, Event, Health,
    LocalRequest, LocalResponse, Machine, MachineMetrics, Process, Recipe, Session, SessionMetrics,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::path::PathBuf;

fn load(name: &str) -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name} is not valid JSON: {e}"))
}

/// Parse `fixture` as `T`, serialise it back, and require identical JSON.
fn assert_round_trip<T: DeserializeOwned + Serialize>(fixture: &Value, name: &str) {
    let parsed: T = serde_json::from_value(fixture.clone())
        .unwrap_or_else(|e| panic!("{name} does not parse: {e}"));
    let back = serde_json::to_value(&parsed).unwrap();
    assert_eq!(&back, fixture, "{name} changed shape after a round trip");
}

fn assert_each_round_trips<T: DeserializeOwned + Serialize>(file: &str) {
    let Value::Array(items) = load(file) else {
        panic!("{file} must be a JSON array");
    };
    assert!(!items.is_empty(), "{file} is empty");
    for (i, item) in items.iter().enumerate() {
        assert_round_trip::<T>(item, &format!("{file}[{i}]"));
    }
}

#[test]
fn machine_contract() {
    assert_round_trip::<Machine>(&load("machine.json"), "machine.json");
}

#[test]
fn session_contract() {
    assert_round_trip::<Session>(&load("session.json"), "session.json");
}

#[test]
fn health_contract() {
    assert_round_trip::<Health>(&load("health.json"), "health.json");
}

#[test]
fn capture_session_contract() {
    assert_each_round_trips::<Session>("sessions_1b.json");
    assert_round_trip::<Session>(&load("session_continued.json"), "session_continued.json");
}

#[test]
fn legacy_session_and_create_request_still_decode() {
    let _: Session = serde_json::from_value(load("session_v1_legacy.json")).unwrap();
    let _: Envelope<ClientRequest> =
        serde_json::from_value(load("create_session_v1_legacy.json")).unwrap();
}

#[test]
fn continuation_contract_is_additive_and_preserves_lineage() {
    let mut continued = load("session.json");
    continued["id"] = Value::from(129);
    continued["continued_from"] = Value::from(128);
    let parsed: Session = serde_json::from_value(continued).unwrap();
    let encoded = serde_json::to_value(parsed).unwrap();
    assert_eq!(encoded["continued_from"], Value::from(128));

    let request = r#"{"version":1,"payload":{"type":"CONTINUE_SESSION","session_id":128}}"#;
    assert!(Envelope::<ClientRequest>::decode(request).is_ok());
}

#[test]
fn running_command_contract() {
    assert_round_trip::<Command>(&load("command_running.json"), "command_running.json");
}

#[test]
fn finished_command_contract() {
    assert_round_trip::<Command>(&load("command_finished.json"), "command_finished.json");
}

#[test]
fn event_contract() {
    assert_each_round_trips::<Event>("events.json");
}

#[test]
fn recipe_contract() {
    assert_round_trip::<Recipe>(&load("recipe.json"), "recipe.json");
}

#[test]
fn process_contract() {
    assert_round_trip::<Process>(&load("process.json"), "process.json");
}

#[test]
fn machine_metrics_contract() {
    assert_round_trip::<MachineMetrics>(&load("machine_metrics.json"), "machine_metrics.json");
}

#[test]
fn session_metrics_contract() {
    assert_round_trip::<SessionMetrics>(&load("session_metrics.json"), "session_metrics.json");
}

#[test]
fn authentication_contract() {
    assert_round_trip::<AuthStatus>(&load("auth_status.json"), "auth_status.json");
    assert_round_trip::<AuthSetupRequest>(
        &load("auth_setup_request.json"),
        "auth_setup_request.json",
    );
    assert_round_trip::<AuthLoginRequest>(
        &load("auth_login_request.json"),
        "auth_login_request.json",
    );
    assert_round_trip::<AuthChangePasswordRequest>(
        &load("auth_change_password_request.json"),
        "auth_change_password_request.json",
    );
    assert_round_trip::<AuthSession>(&load("auth_session.json"), "auth_session.json");
    assert_round_trip::<AuthSessionInfo>(&load("auth_session_info.json"), "auth_session_info.json");
    assert_round_trip::<AuthFailure>(&load("auth_failure.json"), "auth_failure.json");
}

#[test]
fn client_request_contract() {
    assert_each_round_trips::<Envelope<ClientRequest>>("client_requests.json");
}

#[test]
fn local_request_and_response_contract() {
    assert_each_round_trips::<Envelope<LocalRequest>>("local_requests.json");
    assert_each_round_trips::<Envelope<LocalResponse>>("local_responses.json");
}

#[test]
fn every_local_request_has_a_fixture() {
    let all = [
        "REGISTER_SHELL",
        "SHELL_COMMAND_STARTED",
        "SHELL_COMMAND_FINISHED",
        "ADOPT_PROCESS",
    ];
    let requests = load("local_requests.json");
    let present: Vec<&str> = requests
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["payload"]["type"].as_str().unwrap())
        .collect();
    for kind in all {
        assert!(
            present.contains(&kind),
            "local_requests.json has no {kind} example"
        );
    }
}

#[test]
fn daemon_message_contract() {
    assert_each_round_trips::<Envelope<DaemonMessage>>("daemon_messages.json");
}

/// Every event kind must appear in `events.json`, so no kind escapes the contract.
#[test]
fn every_event_kind_has_a_fixture() {
    let all_kinds = [
        "COMMAND_STARTED",
        "COMMAND_FINISHED",
        "PROCESS_STARTED",
        "PROCESS_EXITED",
        "PROGRESS_UPDATED",
        "WARNING_DETECTED",
        "ERROR_DETECTED",
        "RESOURCE_HIGH",
        "WAITING_FOR_INPUT",
        "POSSIBLY_STALLED",
        "CHECKPOINT_CREATED",
        "SERVER_STARTED",
    ];
    let events = load("events.json");
    let present: Vec<&str> = events
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"]["type"].as_str().unwrap())
        .collect();
    for kind in all_kinds {
        assert!(present.contains(&kind), "events.json has no {kind} example");
    }
}

/// Every client request type must appear in `client_requests.json`.
#[test]
fn every_client_request_has_a_fixture() {
    let all = [
        "GET_MACHINE",
        "LIST_SESSIONS",
        "CREATE_SESSION",
        "CONTINUE_SESSION",
        "ATTACH",
        "DETACH",
        "RESIZE",
        "KILL",
        "GET_HISTORY",
        "GET_EVENTS",
        "SEARCH_COMMANDS",
    ];
    let requests = load("client_requests.json");
    let present: Vec<&str> = requests
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["payload"]["type"].as_str().unwrap())
        .collect();
    for kind in all {
        assert!(
            present.contains(&kind),
            "client_requests.json has no {kind} example"
        );
    }
}

/// Every daemon message type must appear in `daemon_messages.json`.
#[test]
fn every_daemon_message_has_a_fixture() {
    let all = [
        "MACHINE",
        "SESSIONS",
        "SESSION_UPDATED",
        "COMMAND_UPDATED",
        "EVENT",
        "EVENTS",
        "MACHINE_METRICS",
        "SESSION_METRICS",
        "HISTORY",
        "ERROR",
    ];
    let messages = load("daemon_messages.json");
    let present: Vec<&str> = messages
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["payload"]["type"].as_str().unwrap())
        .collect();
    for kind in all {
        assert!(
            present.contains(&kind),
            "daemon_messages.json has no {kind} example"
        );
    }
}

/// The recipe fixture must also be a *valid* recipe, not just valid JSON.
#[test]
fn recipe_fixture_renders() {
    let recipe: Recipe = serde_json::from_value(load("recipe.json")).unwrap();
    let cmd = recipe.render(&Default::default()).unwrap();
    assert_eq!(
        cmd,
        "vllm serve Qwen/Qwen3-8B --port 8000 --max-model-len 32768"
    );
}

#[test]
fn every_log_source_has_a_fixture_and_legacy_sessions_have_no_log() {
    let sessions = load("sessions_1b.json");
    let sources: Vec<&str> = sessions
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|session| session["log"]["source"].as_str())
        .collect();
    for source in ["EXPLICIT", "STDOUT", "STDERR", "STDOUT_AND_STDERR"] {
        assert!(
            sources.contains(&source),
            "sessions_1b.json has no {source} log"
        );
    }
    let legacy: Session = serde_json::from_value(load("session_v1_legacy.json")).unwrap();
    assert_eq!(legacy.log, None);
}

// ---- Milestone 4.0: Machine enrollment -------------------------------------

mod machine_link {
    use super::*;
    use protocol::link::{
        LinkDisconnectRequest, LinkExchangeRequest, LinkExchangeResponse, LinkFailure,
        LinkFailureCode, LinkHeartbeatRequest, LinkHeartbeatResponse, LinkIdentity,
        LinkRefreshRequest, LinkRefreshResponse, PairingCode, PairingCodeState, PairingCodeStatus,
        RegistryMachines, RegistryStatus, RenameMachineRequest,
    };
    use protocol::{LocalErrorCode, MachineRole};

    fn section(name: &str) -> Value {
        load("machine_link.json")[name].clone()
    }

    fn each<T: DeserializeOwned + Serialize>(name: &str) -> Vec<T> {
        let Value::Array(items) = section(name) else {
            panic!("{name} must be an array");
        };
        items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                assert_round_trip::<T>(item, &format!("{name}[{index}]"));
                serde_json::from_value(item.clone()).unwrap()
            })
            .collect()
    }

    #[test]
    fn every_link_body_round_trips() {
        assert_round_trip::<PairingCode>(&section("pairing_code"), "pairing_code");
        assert_round_trip::<RegistryMachines>(&section("registry"), "registry");
        assert_round_trip::<RenameMachineRequest>(&section("rename"), "rename");
        assert_round_trip::<LinkExchangeRequest>(&section("exchange_request"), "exchange_request");
        assert_round_trip::<LinkExchangeResponse>(
            &section("exchange_response"),
            "exchange_response",
        );
        assert_round_trip::<LinkHeartbeatRequest>(
            &section("heartbeat_request"),
            "heartbeat_request",
        );
        assert_round_trip::<LinkHeartbeatResponse>(
            &section("heartbeat_response"),
            "heartbeat_response",
        );
        assert_round_trip::<LinkRefreshRequest>(&section("refresh_request"), "refresh_request");
        assert_round_trip::<LinkRefreshResponse>(&section("refresh_response"), "refresh_response");
        assert_round_trip::<LinkDisconnectRequest>(
            &section("disconnect_request"),
            "disconnect_request",
        );
        assert_round_trip::<LinkIdentity>(&section("identity"), "identity");
    }

    #[test]
    fn fixtures_cover_every_state_status_and_failure_code() {
        let states: Vec<PairingCodeState> = each::<PairingCodeStatus>("pairing_code_statuses")
            .into_iter()
            .map(|status| status.state)
            .collect();
        assert_eq!(states, PairingCodeState::ALL.to_vec());

        let registry: RegistryMachines = serde_json::from_value(section("registry")).unwrap();
        let mut statuses: Vec<RegistryStatus> = registry
            .machines
            .iter()
            .map(|machine| machine.status)
            .collect();
        statuses.sort_by_key(|status| format!("{status:?}"));
        statuses.dedup();
        assert_eq!(statuses.len(), RegistryStatus::ALL.len());
        let roles: Vec<MachineRole> = registry
            .machines
            .iter()
            .map(|machine| machine.role)
            .collect();
        assert!(roles.contains(&MachineRole::Control) && roles.contains(&MachineRole::Worker));

        let codes: Vec<LinkFailureCode> = each::<LinkFailure>("failures")
            .into_iter()
            .map(|failure| failure.code)
            .collect();
        assert_eq!(codes, LinkFailureCode::ALL.to_vec());
    }

    #[test]
    fn local_link_requests_and_responses_round_trip() {
        let fixture = load("local_link.json");
        for (index, item) in fixture["requests"].as_array().unwrap().iter().enumerate() {
            assert_round_trip::<Envelope<LocalRequest>>(
                item,
                &format!("local_link.requests[{index}]"),
            );
        }
        let mut errors = Vec::new();
        for (index, item) in fixture["responses"].as_array().unwrap().iter().enumerate() {
            assert_round_trip::<Envelope<LocalResponse>>(
                item,
                &format!("local_link.responses[{index}]"),
            );
            if let LocalResponse::Error { code, .. } =
                serde_json::from_value::<Envelope<LocalResponse>>(item.clone())
                    .unwrap()
                    .payload
            {
                errors.push(code);
            }
        }
        for code in [
            LocalErrorCode::CodeRejected,
            LocalErrorCode::ControlUnreachable,
            LocalErrorCode::AlreadyConnected,
            LocalErrorCode::NotConnected,
            LocalErrorCode::IdentityCorrupt,
        ] {
            assert!(errors.contains(&code), "missing {code:?}");
        }
    }

    #[test]
    fn machine_role_is_optional_and_additive() {
        assert_round_trip::<Machine>(&load("machine_control.json"), "machine_control.json");
        let legacy: Machine = serde_json::from_value(load("machine.json")).unwrap();
        assert_eq!(legacy.role, None);
        assert!(serde_json::to_value(&legacy).unwrap().get("role").is_none());
    }

    #[test]
    fn secrets_never_appear_in_debug_output() {
        let exchange: LinkExchangeRequest =
            serde_json::from_value(section("exchange_request")).unwrap();
        let response: LinkExchangeResponse =
            serde_json::from_value(section("exchange_response")).unwrap();
        let code: PairingCode = serde_json::from_value(section("pairing_code")).unwrap();
        let local: Envelope<LocalRequest> =
            serde_json::from_value(load("local_link.json")["requests"][1].clone()).unwrap();
        for debug in [
            format!("{exchange:?}"),
            format!("{response:?}"),
            format!("{code:?}"),
            format!("{local:?}"),
        ] {
            for secret in ["7KQ4M-2XZ9C", "1111111111", "2222222222"] {
                assert!(!debug.contains(secret), "secret in {debug}");
            }
            assert!(debug.contains("[redacted]"), "{debug}");
        }
    }
}
