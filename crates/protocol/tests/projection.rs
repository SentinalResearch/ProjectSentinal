//! Milestone 4.D.1 projection contract and privacy tests.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use protocol::{
    link::{LinkHeartbeatRequest, LinkHeartbeatResponse},
    projection::{
        ControlProjectionMessage, ControlProjectionSnapshot, ProjectedEventSummary,
        ProjectedSession, ProjectionAcknowledgement, ProjectionBatch,
    },
    CaptureLevel, Event, EventId, EventKind, MachineId, Session, SessionId, Timestamp,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::path::PathBuf;

fn fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/projection.json");
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn round_trip<T: DeserializeOwned + Serialize>(value: &Value) {
    let parsed: T = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), *value);
}

#[test]
fn every_projection_body_round_trips() {
    let fixture = fixture();
    round_trip::<LinkHeartbeatRequest>(&fixture["heartbeat_request"]);
    round_trip::<LinkHeartbeatResponse>(&fixture["heartbeat_response"]);
    round_trip::<ProjectionBatch>(&fixture["delta"]);
    round_trip::<ProjectionBatch>(&fixture["snapshot"]);
    round_trip::<ProjectionAcknowledgement>(&fixture["acknowledgement"]);
    round_trip::<ProjectionAcknowledgement>(&fixture["resync"]);
    round_trip::<ControlProjectionSnapshot>(&fixture["control_snapshot"]);
    round_trip::<ControlProjectionMessage>(&fixture["control_delta"]);
    round_trip::<ControlProjectionMessage>(&fixture["snapshot_required"]);
    round_trip::<ControlProjectionMessage>(&fixture["snapshot_required_registry_changed"]);
}

#[test]
fn legacy_control_delta_without_machine_identity_still_decodes() {
    let value = serde_json::json!({
        "type": "PROJECTION_DELTA",
        "from_revision": 1,
        "to_revision": 2,
        "changes": []
    });
    let decoded: ControlProjectionMessage = serde_json::from_value(value).unwrap();
    assert!(matches!(
        decoded,
        ControlProjectionMessage::ProjectionDelta {
            machine_id: None,
            ..
        }
    ));
}

#[test]
fn an_empty_final_snapshot_can_clear_a_control_projection() {
    let snapshot = ProjectionBatch::Snapshot {
        schema_version: protocol::PROJECTION_SCHEMA_VERSION,
        machine_id: MachineId("machine-a".into()),
        source_epoch: "epoch-a".into(),
        snapshot_id: "snapshot-a".into(),
        snapshot_cursor: 0,
        chunk_index: 0,
        final_chunk: true,
        sent_at: Timestamp(1),
        records: vec![],
    };
    snapshot.validate().unwrap();
}

#[test]
fn legacy_control_snapshot_without_control_identity_still_decodes() {
    let mut value = fixture()["control_snapshot"].clone();
    value.as_object_mut().unwrap().remove("control_machine_id");
    let decoded: ControlProjectionSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(decoded.control_machine_id, None);
}

#[test]
fn legacy_projected_machine_defaults_to_projection_unavailable() {
    let mut value = fixture()["control_snapshot"]["machines"][0].clone();
    value
        .as_object_mut()
        .unwrap()
        .remove("projection_available");
    let decoded: protocol::ControlProjectedMachine = serde_json::from_value(value).unwrap();
    assert!(!decoded.projection_available);
}

#[test]
fn legacy_heartbeat_defaults_to_no_projection_capability() {
    let fixture =
        serde_json::from_str::<Value>(include_str!("fixtures/machine_link.json")).unwrap();
    let request: LinkHeartbeatRequest =
        serde_json::from_value(fixture["heartbeat_request"].clone()).unwrap();
    let response: LinkHeartbeatResponse =
        serde_json::from_value(fixture["heartbeat_response"].clone()).unwrap();
    assert_eq!(request.projection_sync, None);
    assert_eq!(response.projection_sync, None);
}

#[test]
fn event_projection_drops_untrusted_text_and_paths() {
    let events = [
        Event {
            id: EventId(1),
            session_id: SessionId(7),
            command_id: None,
            at: Timestamp(10),
            kind: EventKind::ErrorDetected {
                message: "SECRET_OUTPUT".into(),
            },
        },
        Event {
            id: EventId(2),
            session_id: SessionId(7),
            command_id: None,
            at: Timestamp(11),
            kind: EventKind::CheckpointCreated {
                path: "/secret/model.safetensors".into(),
            },
        },
        Event {
            id: EventId(3),
            session_id: SessionId(7),
            command_id: None,
            at: Timestamp(12),
            kind: EventKind::WaitingForInput {
                prompt: Some("PASSWORD_PROMPT".into()),
                marked: None,
                options: Vec::new(),
            },
        },
    ];

    let json = serde_json::to_string(
        &events
            .iter()
            .map(ProjectedEventSummary::from_event)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for forbidden in ["SECRET_OUTPUT", "/secret", "PASSWORD_PROMPT"] {
        assert!(!json.contains(forbidden), "projection leaked {forbidden}");
    }
}

#[test]
fn session_projection_contains_no_command_cwd_shell_pid_or_health_evidence() {
    let mut session = Session::new(
        SessionId(9),
        MachineId("machine-a".into()),
        "/secret/shell",
        "/secret/cwd",
        Timestamp(10),
    );
    session.title = Some("Training".into());
    session.pid = Some(4242);
    session.health = Some(protocol::Health {
        progress: Some(50.0),
        activity: protocol::HealthLevel::Good,
        resources: protocol::HealthLevel::Good,
        stability: protocol::HealthLevel::Good,
        errors: 0,
        overall: protocol::HealthOverall::Healthy,
        evidence: vec!["SECRET_EVIDENCE".into()],
    });

    let projected = ProjectedSession::from_session(&session);
    let json = serde_json::to_string(&projected).unwrap();
    for forbidden in ["/secret/shell", "/secret/cwd", "4242", "SECRET_EVIDENCE"] {
        assert!(!json.contains(forbidden), "projection leaked {forbidden}");
    }
    assert_eq!(projected.title.as_deref(), Some("Training"));
}

#[test]
fn capture_display_capabilities_cover_every_level() {
    for capture in [
        CaptureLevel::Full,
        CaptureLevel::Commands,
        CaptureLevel::ProcessLog,
        CaptureLevel::Process,
    ] {
        let projected = protocol::ProjectedCaptureCapabilities::from(capture);
        let source = capture.capabilities();
        assert_eq!(projected.can_attach, source.can_attach);
        assert_eq!(projected.can_send_input, source.can_send_input);
        assert_eq!(projected.can_resize, source.can_resize);
        assert_eq!(projected.can_kill_daemon_pty, source.can_kill_daemon_pty);
        assert_eq!(projected.exposes_commands, source.exposes_commands);
        assert_eq!(projected.exposes_metrics, source.exposes_metrics);
    }
}

#[test]
fn projection_batch_rejects_bad_cursors_and_excess_changes() {
    let fixture = fixture();
    let mut delta: ProjectionBatch = serde_json::from_value(fixture["delta"].clone()).unwrap();
    assert!(delta.validate().is_ok());
    if let ProjectionBatch::Delta {
        base_cursor,
        through_cursor,
        ..
    } = &mut delta
    {
        *base_cursor = *through_cursor;
    }
    assert!(delta.validate().is_err());
    if let ProjectionBatch::Delta {
        base_cursor,
        changes,
        ..
    } = &mut delta
    {
        *base_cursor = 41;
        while changes.len() <= protocol::MAX_PROJECTION_CHANGES {
            changes.push(changes[0].clone());
        }
    }
    assert!(delta.validate().is_err());
}

#[test]
fn only_known_metric_names_reach_the_control() {
    use protocol::{
        Event, EventId, EventKind, ProjectedEventFacts, ProjectedEventSummary, SessionId, Timestamp,
    };
    let summary = |name: &str| {
        ProjectedEventSummary::from_event(&Event {
            id: EventId(1),
            session_id: SessionId(1),
            command_id: None,
            at: Timestamp(1),
            kind: EventKind::MetricReported {
                name: name.into(),
                value: 0.5,
            },
        })
        .facts
    };
    assert_eq!(
        summary("Loss"),
        ProjectedEventFacts::MetricReported {
            name: "loss".into(),
            value: 0.5
        }
    );
    // A name the program made up could carry anything; the control never sees it.
    assert_eq!(
        summary("my_private_project_codename"),
        ProjectedEventFacts::MetricReported {
            name: "metric".into(),
            value: 0.5
        }
    );
}

#[test]
fn projection_carries_the_agent_state_and_sandbox_but_never_a_bad_detail() {
    let mut session = Session::new(
        SessionId(9),
        MachineId("machine-a".into()),
        "/bin/zsh",
        "/work",
        Timestamp(10),
    );
    session.sandbox = true;
    session.agent_state = Some(Box::new(protocol::AgentReportedState {
        detail: Some("needs approval for cargo test".into()),
        task: Some("migrate tests".into()),
        step: Some(3),
        steps: Some(8),
        eta_secs: Some(720),
        task_started_at: Some(Timestamp(12)),
        estimated_left_secs: Some(900),
        ..protocol::AgentReportedState::new(protocol::AgentStatus::Blocked, Timestamp(20))
    }));
    let projected = ProjectedSession::from_session(&session);
    assert!(projected.sandbox);
    assert_eq!(
        projected.agent_state.as_ref(),
        session.agent_state.as_deref()
    );

    // A detail that slipped past validation is dropped, the status kept.
    session.agent_state.as_mut().unwrap().detail = Some("bad\x1b]0;title\x07".into());
    session.agent_state.as_mut().unwrap().task = Some("bad\x1b[2Jtask".into());
    let projected = ProjectedSession::from_session(&session);
    let state = projected.agent_state.unwrap();
    assert_eq!(state.status, protocol::AgentStatus::Blocked);
    assert_eq!(state.detail, None);
    assert_eq!(state.task, None, "a bad task is dropped, the steps kept");
    assert_eq!(state.step, Some(3));
}
