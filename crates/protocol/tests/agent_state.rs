//! Milestone 6.4: an agent's own state report, on the wire and on the Session.

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
        } => {
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
