//! The agent tool catalogue (Milestone 6.0) and the read-only tools that
//! answer from the control's structured read model.
//!
//! The catalogue is the single source for `tools/list` and for
//! `docs/agent-tools.json`. Every tool here needs level 0 (observe); tools
//! that annotate or suggest arrive with the daemon requests they need.

use protocol::{
    agent::AgentIdentity, ControlProjectedMachine, ControlProjectionSnapshot, ProjectedSession,
    SessionState,
};
use serde_json::{json, Value};

/// Longest `wait_for` may block.
pub const MAX_WAIT_FOR_SECONDS: u64 = 600;
/// Longest one `watch` call may block.
pub const MAX_WATCH_SECONDS: u64 = 30;

const STATES: [&str; 9] = [
    "CREATED",
    "IDLE",
    "RUNNING",
    "WAITING",
    "STALLED",
    "COMPLETED",
    "FAILED",
    "INTERRUPTED",
    "DISCONNECTED",
];

/// Every tool, as MCP `tools/list` returns them. Each carries its level.
pub fn catalogue() -> Value {
    let machine =
        json!({"type": "string", "description": "Machine ID or display name (case-insensitive)."});
    let session =
        json!({"type": "integer", "minimum": 1, "description": "Session ID on that Machine."});
    let state = json!({"type": "string", "enum": STATES});
    json!([
        tool("whoami", 0, "Who this agent is and what its level allows.", json!({})),
        tool(
            "list_machines",
            0,
            "Machines with their connection status and how many Sessions run or need a person.",
            json!({}),
        ),
        tool(
            "list_sessions",
            0,
            "Sessions across Machines with state, capture level, health, and timing. Filter by Machine, state, or needs_you (WAITING, STALLED, or FAILED).",
            json!({"machine": machine, "state": state, "needs_you": {"type": "boolean"}}),
        ),
        tool(
            "get_session",
            0,
            "One Session with its recent Events (progress, metrics, checkpoints, errors, prompts) and resource use. Structured facts only; never terminal output.",
            with_required(json!({"machine": machine, "session_id": session}), &["machine", "session_id"]),
        ),
        tool(
            "watch",
            0,
            "Changes since a revision. Call without after_revision to get the current one, then pass the returned revision back. Waits up to wait_seconds (max 30) for a change.",
            json!({"after_revision": {"type": "integer", "minimum": 0}, "wait_seconds": {"type": "integer", "minimum": 0, "maximum": MAX_WATCH_SECONDS}}),
        ),
        tool(
            "wait_for",
            0,
            "Block until a Session reaches one of the given states, or the timeout (max 600 s) passes. Use instead of polling.",
            with_required(
                json!({
                    "machine": machine,
                    "session_id": session,
                    "until": {"type": "array", "items": state, "minItems": 1},
                    "timeout_seconds": {"type": "integer", "minimum": 1, "maximum": MAX_WAIT_FOR_SECONDS}
                }),
                &["machine", "session_id", "until"],
            ),
        ),
    ])
}

fn tool(name: &str, level: u8, description: &str, properties: Value) -> Value {
    let schema = if properties.get("type").is_some() {
        properties
    } else {
        json!({"type": "object", "properties": properties, "additionalProperties": false})
    };
    json!({
        "name": name,
        "description": format!("{description} [level {level}]"),
        "inputSchema": schema,
        "annotations": {"readOnlyHint": true, "openWorldHint": false},
    })
}

fn with_required(properties: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": properties, "required": required, "additionalProperties": false})
}

/// A tool failure the agent can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolError(pub String);

fn invalid(message: impl Into<String>) -> ToolError {
    ToolError(message.into())
}

fn state_name(state: SessionState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// Whether a person should look at this Session now.
pub fn needs_you(state: SessionState) -> bool {
    matches!(
        state,
        SessionState::Waiting | SessionState::Stalled | SessionState::Failed
    )
}

pub(crate) fn whoami(identity: &AgentIdentity) -> Value {
    let may = match identity.level.number() {
        0 => "read structured state and watch it change",
        1 => "read and watch; add notes and proposed Events",
        _ => "read and watch; add notes; suggest commands and answers a person must accept",
    };
    json!({"id": identity.id, "name": identity.name, "level": identity.level.number(), "may": may,
           "never": "type into a terminal, run commands, or read terminal output unless the owner enables it"})
}

pub(crate) fn list_machines(snapshot: &ControlProjectionSnapshot) -> Value {
    let machines: Vec<Value> = snapshot
        .machines
        .iter()
        .map(|machine| {
            let running = machine
                .sessions
                .iter()
                .filter(|s| s.state == SessionState::Running)
                .count();
            let waiting = machine
                .sessions
                .iter()
                .filter(|s| needs_you(s.state))
                .count();
            json!({
                "machine_id": machine.machine_id.0,
                "name": machine.display_name,
                "os": machine.os,
                "status": machine.freshness,
                "last_seen": machine.last_seen,
                "sessions": machine.sessions.len(),
                "running": running,
                "needs_you": waiting,
            })
        })
        .collect();
    json!({"revision": snapshot.view_revision, "machines": machines, "truncated": snapshot.truncated})
}

fn session_summary(machine: &ControlProjectedMachine, session: &ProjectedSession) -> Value {
    json!({
        "machine": machine.display_name,
        "machine_id": machine.machine_id.0,
        "session_id": session.id.0,
        "title": session.title,
        "state": session.state,
        "needs_you": needs_you(session.state),
        "capture": session.capture,
        "health": session.health,
        "created_at": session.created_at,
        "updated_at": session.updated_at,
        "finished_at": session.finished_at,
        "exit_code": session.exit_code,
    })
}

fn find_machine<'a>(
    snapshot: &'a ControlProjectionSnapshot,
    wanted: &str,
) -> Result<&'a ControlProjectedMachine, ToolError> {
    snapshot
        .machines
        .iter()
        .find(|m| m.machine_id.0 == wanted)
        .or_else(|| {
            snapshot
                .machines
                .iter()
                .find(|m| m.display_name.eq_ignore_ascii_case(wanted))
        })
        .ok_or_else(|| {
            let known: Vec<&str> = snapshot
                .machines
                .iter()
                .map(|m| m.display_name.as_str())
                .collect();
            invalid(format!(
                "no Machine '{wanted}'; known: {}",
                known.join(", ")
            ))
        })
}

fn optional_str<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(invalid(format!("{key} must be a string"))),
    }
}

fn required_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    optional_str(args, key)?.ok_or_else(|| invalid(format!("{key} is required")))
}

pub(crate) fn required_u64(args: &Value, key: &str) -> Result<u64, ToolError> {
    optional_u64(args, key)?.ok_or_else(|| invalid(format!("{key} is required")))
}

pub(crate) fn optional_u64(args: &Value, key: &str) -> Result<Option<u64>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| invalid(format!("{key} must be a non-negative integer"))),
    }
}

pub(crate) fn parse_states(args: &Value) -> Result<Vec<SessionState>, ToolError> {
    let list = args
        .get("until")
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
        .ok_or_else(|| invalid("until must list at least one state"))?;
    list.iter()
        .map(|value| {
            serde_json::from_value(value.clone()).map_err(|_| {
                invalid(format!(
                    "unknown state {value}; use one of {}",
                    STATES.join(", ")
                ))
            })
        })
        .collect()
}

pub(crate) fn list_sessions(
    snapshot: &ControlProjectionSnapshot,
    args: &Value,
) -> Result<Value, ToolError> {
    let machine = optional_str(args, "machine")?
        .map(|wanted| find_machine(snapshot, wanted))
        .transpose()?;
    let state: Option<SessionState> = optional_str(args, "state")?
        .map(|value| {
            serde_json::from_value(json!(value))
                .map_err(|_| invalid(format!("unknown state '{value}'")))
        })
        .transpose()?;
    let needs = match args.get("needs_you") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => return Err(invalid("needs_you must be true or false")),
    };
    let sessions: Vec<Value> = snapshot
        .machines
        .iter()
        .filter(|m| machine.is_none_or(|wanted| wanted.machine_id == m.machine_id))
        .flat_map(|m| m.sessions.iter().map(move |s| (m, s)))
        .filter(|(_, s)| state.is_none_or(|wanted| s.state == wanted))
        .filter(|(_, s)| needs.is_none_or(|wanted| needs_you(s.state) == wanted))
        .map(|(m, s)| session_summary(m, s))
        .collect();
    Ok(json!({"revision": snapshot.view_revision, "sessions": sessions}))
}

pub(crate) fn find_session<'a>(
    snapshot: &'a ControlProjectionSnapshot,
    args: &Value,
) -> Result<(&'a ControlProjectedMachine, &'a ProjectedSession), ToolError> {
    let machine = find_machine(snapshot, required_str(args, "machine")?)?;
    let id = required_u64(args, "session_id")?;
    let session = machine
        .sessions
        .iter()
        .find(|s| s.id.0 == id)
        .ok_or_else(|| invalid(format!("no Session {id} on {}", machine.display_name)))?;
    Ok((machine, session))
}

pub(crate) fn get_session(
    snapshot: &ControlProjectionSnapshot,
    args: &Value,
) -> Result<Value, ToolError> {
    let (machine, session) = find_session(snapshot, args)?;
    let events: Vec<&protocol::ProjectedEventSummary> = machine
        .events
        .iter()
        .filter(|event| event.session_id == session.id)
        .collect();
    let metrics = machine
        .metrics
        .iter()
        .filter(|metric| metric.session_id == Some(session.id))
        .max_by_key(|metric| metric.at);
    let mut value = session_summary(machine, session);
    value["state_word"] = json!(state_name(session.state).to_lowercase());
    value["events"] = json!(events);
    value["resources"] = json!(metrics);
    value["revision"] = json!(snapshot.view_revision);
    Ok(value)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn snapshot() -> ControlProjectionSnapshot {
        serde_json::from_value(json!({
            "schema_version": 1, "control_machine_id": "01J9Y0C7RQ9M3X8V6T4N2P0SDB", "view_revision": 42,
            "generated_at": 1_793_539_400_000_i64, "omitted_machines": 0, "omitted_sessions": 0, "truncated": false,
            "machines": [machine("01J9Z3K8V6Q2T7M4X5C0RNB8DA", "pop-os", &[(7, "RUNNING"), (8, "WAITING")]),
                         machine("01J9Z6BBBBBBBBBBBBBBBBBBBB", "rtx-pc", &[(3, "FAILED"), (4, "COMPLETED")])]
        }))
        .unwrap()
    }

    fn machine(id: &str, name: &str, sessions: &[(u64, &str)]) -> Value {
        let sessions: Vec<Value> = sessions
            .iter()
            .map(|(sid, state)| {
                let mut session = protocol::Session::new(
                    protocol::SessionId(*sid),
                    protocol::MachineId(id.into()),
                    "/bin/zsh",
                    "/home/u",
                    protocol::Timestamp(1),
                );
                session.title = Some(format!("job {sid}"));
                let mut value =
                    serde_json::to_value(ProjectedSession::from_session(&session)).unwrap();
                value["state"] = json!(state);
                value
            })
            .collect();
        json!({"machine_id": id, "projection_available": true, "display_name": name, "os": "LINUX",
               "daemon_version": "0.3.0", "capabilities": ["SESSIONS"], "registry_status": "ONLINE",
               "freshness": "LIVE", "last_seen": 1, "observed_at": 1, "synced_at": 1, "source_cursor": 1,
               "sessions": sessions, "events": [], "metrics": [], "omitted_sessions": 0})
    }

    #[test]
    fn every_tool_is_read_only_level_zero_with_an_object_schema() {
        let tools = catalogue();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "whoami",
                "list_machines",
                "list_sessions",
                "get_session",
                "watch",
                "wait_for"
            ]
        );
        for tool in tools.as_array().unwrap() {
            assert_eq!(tool["inputSchema"]["type"], "object", "{tool}");
            assert_eq!(tool["annotations"]["readOnlyHint"], true);
            assert!(tool["description"].as_str().unwrap().ends_with("[level 0]"));
        }
    }

    #[test]
    fn machines_count_what_needs_a_person() {
        let value = list_machines(&snapshot());
        assert_eq!(value["revision"], 42);
        assert_eq!(value["machines"][0]["name"], "pop-os");
        assert_eq!(
            (
                value["machines"][0]["running"].as_u64(),
                value["machines"][0]["needs_you"].as_u64()
            ),
            (Some(1), Some(1))
        );
        assert_eq!(value["machines"][1]["needs_you"], 1);
    }

    #[test]
    fn sessions_filter_by_machine_name_state_and_needs_you() {
        let snap = snapshot();
        let all = list_sessions(&snap, &json!({})).unwrap();
        assert_eq!(all["sessions"].as_array().unwrap().len(), 4);
        let pop = list_sessions(&snap, &json!({"machine": "POP-OS"})).unwrap();
        assert_eq!(pop["sessions"].as_array().unwrap().len(), 2);
        let needs = list_sessions(&snap, &json!({"needs_you": true})).unwrap();
        let ids: Vec<u64> = needs["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["session_id"].as_u64().unwrap())
            .collect();
        assert_eq!(ids, [8, 3]);
        let failed = list_sessions(&snap, &json!({"state": "FAILED"})).unwrap();
        assert_eq!(failed["sessions"][0]["machine"], "rtx-pc");
    }

    #[test]
    fn bad_arguments_say_what_to_fix() {
        let snap = snapshot();
        assert!(list_sessions(&snap, &json!({"machine": "mars"}))
            .unwrap_err()
            .0
            .contains("known: pop-os, rtx-pc"));
        assert!(list_sessions(&snap, &json!({"state": "EXPLODED"})).is_err());
        assert!(get_session(&snap, &json!({"machine": "pop-os"}))
            .unwrap_err()
            .0
            .contains("session_id is required"));
        assert!(
            get_session(&snap, &json!({"machine": "pop-os", "session_id": 99}))
                .unwrap_err()
                .0
                .contains("no Session 99")
        );
        assert!(parse_states(&json!({"until": []})).is_err());
        assert!(parse_states(&json!({"until": ["DONE"]})).is_err());
    }

    #[test]
    fn get_session_carries_structured_facts_only() {
        let value = get_session(
            &snapshot(),
            &json!({"machine": "01J9Z3K8V6Q2T7M4X5C0RNB8DA", "session_id": 8}),
        )
        .unwrap();
        assert_eq!(value["state"], "WAITING");
        assert_eq!(value["needs_you"], true);
        assert!(value.get("output").is_none() && value.get("screen").is_none());
    }
}
