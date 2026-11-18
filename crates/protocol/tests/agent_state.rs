//! Milestones 6.4 and 6.5: an agent's own state report and progress, on the
//! wire and on the Session.

use protocol::{AgentStatus, Envelope, LocalRequest, Session, SessionId};

#[test]
fn legacy_session_json_has_no_agent_state_and_is_not_a_sandbox() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/session.json"
    ))
    .unwrap();
    let session: Session = serde_json::from_str(&text).unwrap();
    assert_eq!(session.agent_state, None);
    assert!(!session.sandbox);
    let back = serde_json::to_value(&session).unwrap();
    assert!(
        back.get("agent_state").is_none(),
        "absent field stays absent"
    );
    assert!(back.get("sandbox").is_none(), "false is not written");
}

#[test]
fn a_report_decodes_without_any_process_id_from_the_wire() {
    // A client that tries to name the calling process is ignored: only the
    // socket layer, from OS credentials, may set it.
    let json = r#"{"version":1,"payload":{"type":"AGENT_STATE","session_id":7,
        "status":"WORKING","detail":"migrating tests","caller_pid":1}}"#;
    let envelope = Envelope::<LocalRequest>::decode(json).unwrap();
    match envelope.payload {
        LocalRequest::AgentState {
            session_id,
            status,
            detail,
            caller_pid,
            task,
            ..
        } => {
            assert_eq!(task, None, "a 6.4 report carries no progress");
            assert_eq!(session_id, SessionId(7));
            assert_eq!(status, AgentStatus::Working);
            assert_eq!(detail.as_deref(), Some("migrating tests"));
            assert_eq!(caller_pid, None);
        }
        other => panic!("decoded as {other:?}"),
    }
}

#[test]
fn the_process_id_is_never_written_to_the_wire() {
    let request = LocalRequest::AgentState {
        session_id: SessionId(7),
        status: AgentStatus::Done,
        detail: None,
        task: None,
        step: None,
        steps: None,
        eta_secs: None,
        caller_pid: Some(4242),
    };
    let json = serde_json::to_string(&request).unwrap();
    assert!(
        !json.contains("4242") && !json.contains("caller_pid"),
        "{json}"
    );
}

#[test]
fn reports_with_a_zero_session_or_a_bad_detail_are_refused() {
    let report = |session_id: u64, detail: Option<&str>| LocalRequest::AgentState {
        session_id: SessionId(session_id),
        status: AgentStatus::Blocked,
        detail: detail.map(str::to_string),
        task: None,
        step: None,
        steps: None,
        eta_secs: None,
        caller_pid: None,
    };
    assert!(report(7, Some("needs the staging key")).validate().is_ok());
    assert!(report(0, None).validate().is_err());
    assert!(report(7, Some("bell\x07")).validate().is_err());
    assert!(report(7, Some(&"x".repeat(201))).validate().is_err());
}

#[test]
fn only_a_watch_session_can_be_a_sandbox() {
    let request = |origin: protocol::SessionOrigin| protocol::ClientRequest::CreateSession {
        shell: None,
        cwd: None,
        cols: 80,
        rows: 24,
        origin,
        title: None,
        command: Some(vec!["make".into()]),
        env: None,
        sandbox: true,
    };
    assert!(request(protocol::SessionOrigin::Watch).validate().is_ok());
    assert_eq!(
        request(protocol::SessionOrigin::Ui).validate(),
        Err(protocol::ProtocolError::SandboxNeedsWatch)
    );
}

#[test]
fn a_create_request_without_sandbox_decodes_as_not_a_sandbox() {
    let json = r#"{"type":"CREATE_SESSION","shell":null,"cwd":"/w","cols":80,"rows":24}"#;
    let request: protocol::ClientRequest = serde_json::from_str(json).unwrap();
    assert!(matches!(
        request,
        protocol::ClientRequest::CreateSession { sandbox: false, .. }
    ));
}

// ---- Milestone 6.5: task, steps and ETA ----

fn progress(
    task: Option<&str>,
    step: Option<u32>,
    steps: Option<u32>,
    eta_secs: Option<u32>,
) -> LocalRequest {
    LocalRequest::AgentState {
        session_id: SessionId(7),
        status: AgentStatus::Working,
        detail: None,
        task: task.map(str::to_string),
        step,
        steps,
        eta_secs,
        caller_pid: None,
    }
}

#[test]
fn a_progress_report_decodes_with_every_field() {
    let json = r#"{"version":1,"payload":{"type":"AGENT_STATE","session_id":7,
        "status":"WORKING","task":"migrate tests","step":3,"steps":8,"eta_secs":720}}"#;
    let request = Envelope::<LocalRequest>::decode(json).unwrap().payload;
    assert_eq!(
        request,
        progress(Some("migrate tests"), Some(3), Some(8), Some(720))
    );
    assert!(request.validate().is_ok());
}

#[test]
fn each_progress_field_is_optional_on_its_own() {
    assert!(progress(Some("migrate tests"), None, None, None)
        .validate()
        .is_ok());
    assert!(progress(None, Some(3), None, None).validate().is_ok());
    assert!(progress(None, None, Some(8), None).validate().is_ok());
    assert!(progress(None, None, None, Some(60)).validate().is_ok());
    assert!(progress(None, Some(0), Some(0), None).validate().is_ok());
}

#[test]
fn progress_fields_outside_their_bounds_are_refused() {
    assert!(progress(Some(""), None, None, None).validate().is_err());
    assert!(progress(Some(&"t".repeat(81)), None, None, None)
        .validate()
        .is_err());
    assert!(progress(Some(&"t".repeat(80)), None, None, None)
        .validate()
        .is_ok());
    assert!(progress(Some("tab\there"), None, None, None)
        .validate()
        .is_err());
    assert!(progress(None, Some(9), Some(8), None).validate().is_err());
    assert!(progress(None, None, Some(10_001), None).validate().is_err());
    assert!(progress(None, Some(10_001), None, None).validate().is_err());
    assert!(progress(None, Some(10_000), Some(10_000), None)
        .validate()
        .is_ok());
    assert!(progress(None, None, None, Some(7 * 24 * 3600))
        .validate()
        .is_ok());
    assert!(progress(None, None, None, Some(7 * 24 * 3600 + 1))
        .validate()
        .is_err());
}

#[test]
fn the_daemon_only_fields_cannot_be_sent_by_an_agent() {
    // task_started_at and estimated_left_secs exist only on the Session; a
    // request naming them still decodes, and they are simply not there.
    let json = r#"{"version":1,"payload":{"type":"AGENT_STATE","session_id":7,
        "status":"WORKING","task_started_at":1,"estimated_left_secs":5}}"#;
    let request = Envelope::<LocalRequest>::decode(json).unwrap().payload;
    let back = serde_json::to_string(&request).unwrap();
    assert!(
        !back.contains("task_started_at") && !back.contains("estimated_left_secs"),
        "{back}"
    );
}

#[test]
fn a_6_4_session_report_decodes_without_progress() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/session_agent_state.json"
    ))
    .unwrap();
    let session: Session = serde_json::from_str(&text).unwrap();
    let state = session.agent_state.unwrap();
    assert_eq!(state.task, None);
    assert_eq!(state.step, None);
    assert_eq!(state.task_started_at, None);
    assert_eq!(state.estimated_left_secs, None);
}

#[test]
fn an_eta_is_written_in_seconds_minutes_or_hours() {
    use protocol::parse_agent_eta;
    assert_eq!(parse_agent_eta("90s"), Some(90));
    assert_eq!(parse_agent_eta("12m"), Some(720));
    assert_eq!(parse_agent_eta("2h"), Some(7_200));
    assert_eq!(parse_agent_eta("168h"), Some(7 * 24 * 3600));
    assert_eq!(parse_agent_eta("169h"), None, "over 7 days");
    for bad in [
        "",
        "12",
        "m",
        "-5m",
        "1.5h",
        "12 m",
        "3d",
        "99999999999999h",
    ] {
        assert_eq!(parse_agent_eta(bad), None, "{bad}");
    }
}
