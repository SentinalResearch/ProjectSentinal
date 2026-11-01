use super::*;
use protocol::agent::AgentLevel;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
};

/// A control whose Session 8 on pop-os becomes COMPLETED after `finish_after` snapshots.
struct FakeControl {
    snapshots: Cell<u32>,
    finish_after: u32,
    updates_calls: RefCell<Vec<(u64, Duration)>>,
}

impl FakeControl {
    fn new(finish_after: u32) -> Self {
        FakeControl {
            snapshots: Cell::new(0),
            finish_after,
            updates_calls: RefCell::new(Vec::new()),
        }
    }
}

impl Backend for FakeControl {
    async fn whoami(&self) -> Result<AgentIdentity, String> {
        Ok(AgentIdentity {
            id: "agt_3f9c1a2b4d5e6f70".into(),
            name: "claude".into(),
            level: AgentLevel::Observe,
            output_access: false,
        })
    }

    async fn snapshot(&self) -> Result<ControlProjectionSnapshot, String> {
        let count = self.snapshots.get() + 1;
        self.snapshots.set(count);
        let mut snapshot = tools::tests::snapshot();
        snapshot.view_revision = 40 + u64::from(count);
        if count > self.finish_after {
            snapshot.machines[0].sessions[1].state = protocol::SessionState::Completed;
        }
        Ok(snapshot)
    }

    async fn updates(&self, after: u64, wait: Duration) -> Result<AgentUpdates, String> {
        self.updates_calls.borrow_mut().push((after, wait));
        tokio::time::sleep(wait.min(Duration::from_secs(1))).await;
        Ok(AgentUpdates { messages: vec![] })
    }
}

async fn rpc(backend: &FakeControl, method: &str, params: Value) -> Value {
    handle(
        backend,
        json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}),
    )
    .await
    .unwrap()
}

async fn tool(backend: &FakeControl, name: &str, arguments: Value) -> (Value, bool) {
    let reply = rpc(
        backend,
        "tools/call",
        json!({"name": name, "arguments": arguments}),
    )
    .await;
    let result = &reply["result"];
    (
        result["structuredContent"].clone(),
        result["isError"].as_bool().unwrap(),
    )
}

#[tokio::test]
async fn initialize_and_list_tools_follow_mcp() {
    let backend = FakeControl::new(0);
    let reply = rpc(
        &backend,
        "initialize",
        json!({"protocolVersion": "2025-03-26", "capabilities": {}}),
    )
    .await;
    assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
    assert_eq!(reply["result"]["serverInfo"]["name"], "sentinal");
    assert!(reply["result"]["capabilities"]["tools"].is_object());
    let list = rpc(&backend, "tools/list", json!({})).await;
    assert_eq!(list["result"]["tools"], tools::catalogue());
    let notification = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
    assert_eq!(handle(&backend, notification).await, None);
    assert_eq!(
        rpc(&backend, "resources/list", json!({})).await["error"]["code"],
        -32601
    );
}

#[tokio::test]
async fn unknown_tools_are_protocol_errors_and_bad_arguments_are_tool_errors() {
    let backend = FakeControl::new(0);
    let reply = rpc(
        &backend,
        "tools/call",
        json!({"name": "send_input", "arguments": {"text": "rm -rf ~"}}),
    )
    .await;
    assert_eq!(reply["error"]["code"], -32602);
    let (value, is_error) = tool(&backend, "get_session", json!({"machine": "pop-os"})).await;
    assert!(is_error);
    assert!(value["error"]
        .as_str()
        .unwrap()
        .contains("session_id is required"));
}

#[tokio::test]
async fn whoami_and_sessions_answer_from_the_control() {
    let backend = FakeControl::new(u32::MAX);
    let (who, _) = tool(&backend, "whoami", json!({})).await;
    assert_eq!(
        (who["name"].as_str(), who["level"].as_u64()),
        (Some("claude"), Some(0))
    );
    let (sessions, is_error) = tool(&backend, "list_sessions", json!({"needs_you": true})).await;
    assert!(!is_error);
    assert_eq!(sessions["sessions"].as_array().unwrap().len(), 2);
}

#[tokio::test(start_paused = true)]
async fn wait_for_returns_when_the_session_reaches_a_state() {
    let backend = FakeControl::new(2);
    let (value, is_error) = tool(&backend, "wait_for",
        json!({"machine": "pop-os", "session_id": 8, "until": ["COMPLETED", "FAILED"], "timeout_seconds": 120})).await;
    assert!(!is_error, "{value}");
    assert_eq!(value["matched"], true);
    assert_eq!(value["state"], "COMPLETED");
    let calls = backend.updates_calls.borrow();
    assert_eq!(
        calls.len(),
        2,
        "it waited on updates between checks instead of polling"
    );
    assert_eq!(
        calls[0].0, 41,
        "each wait starts from the snapshot it just read"
    );
}

#[tokio::test(start_paused = true)]
async fn wait_for_gives_up_at_the_timeout_with_the_current_state() {
    let backend = FakeControl::new(u32::MAX);
    let (value, _) = tool(
        &backend,
        "wait_for",
        json!({"machine": "pop-os", "session_id": 8, "until": ["COMPLETED"], "timeout_seconds": 3}),
    )
    .await;
    assert_eq!(value["matched"], false);
    assert_eq!(value["state"], "WAITING");
    assert!(value["waited_seconds"].as_u64().unwrap() >= 3);
}

#[tokio::test]
async fn watch_starts_from_the_current_revision() {
    let backend = FakeControl::new(0);
    let (value, _) = tool(&backend, "watch", json!({})).await;
    assert_eq!(value["revision"], 41);
    let (value, _) = tool(
        &backend,
        "watch",
        json!({"after_revision": 41, "wait_seconds": 0}),
    )
    .await;
    assert_eq!(
        (
            value["revision"].as_u64(),
            value["changes"].as_array().unwrap().len()
        ),
        (Some(41), 0)
    );
}

#[tokio::test]
async fn the_stdio_loop_answers_each_line_and_reports_bad_json() {
    let backend = FakeControl::new(0);
    let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\nnot json\n\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";
    let mut output = Vec::new();
    serve(&backend, &input[..], &mut output).await.unwrap();
    let lines: Vec<Value> = String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["result"], json!({}));
    assert_eq!(lines[1]["error"]["code"], -32700);
}

#[test]
fn the_published_catalogue_is_current() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/agent-tools.json");
    let expected = serde_json::to_string_pretty(&json!({
        "about": "Sentinal agent tools (Milestone 6.0). Generated from crates/cli/src/mcp/tools.rs; do not edit.",
        "tools": tools::catalogue(),
    }))
    .unwrap()
        + "\n";
    if std::env::var_os("UPDATE_AGENT_TOOLS").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let actual = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(actual == expected, "docs/agent-tools.json is stale; run UPDATE_AGENT_TOOLS=1 cargo test -p cli the_published_catalogue_is_current");
}
