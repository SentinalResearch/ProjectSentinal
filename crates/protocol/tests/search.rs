//! Milestone 5.S: search contract and the ranking shared with the browser.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use protocol::{
    search::{
        clip, match_score, rank_score, SearchFilters, SearchHit, SearchOutcome, SearchScope,
        SearchTarget, MAX_SEARCH_LIMIT, MAX_SEARCH_QUERY_BYTES,
    },
    ClientRequest, CommandId, DaemonMessage, EventId, ProtocolError, SessionId, Timestamp,
};
use serde_json::{json, Value};

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/search_ranking.json")).unwrap()
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_string())
        .collect()
}

#[test]
fn match_scores_follow_the_shared_table() {
    for case in fixture()["match"].as_array().unwrap() {
        let query = case["query"].as_str().unwrap();
        let haystacks = strings(&case["haystacks"]);
        let refs: Vec<&str> = haystacks.iter().map(String::as_str).collect();
        assert_eq!(
            match_score(query, &refs),
            case["score"].as_f64().unwrap(),
            "{query} in {haystacks:?}"
        );
    }
}

#[test]
fn rank_scores_follow_the_shared_table() {
    for case in fixture()["rank"].as_array().unwrap() {
        let score = rank_score(
            case["match"].as_f64().unwrap(),
            case["age_ms"].as_i64().unwrap(),
            case["needs_you"].as_bool().unwrap(),
        );
        assert_eq!(score, case["score"].as_f64().unwrap(), "{case}");
    }
}

#[test]
fn orderings_follow_the_shared_table() {
    for case in fixture()["order"].as_array().unwrap() {
        let query = case["query"].as_str().unwrap();
        let mut items: Vec<(String, f64)> = case["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| {
                let text = item["text"].as_str().unwrap().to_string();
                let score = rank_score(
                    match_score(query, &[text.as_str()]),
                    item["age_ms"].as_i64().unwrap(),
                    item["needs_you"].as_bool().unwrap(),
                );
                (text, score)
            })
            .collect();
        items.sort_by(|a, b| b.1.total_cmp(&a.1));
        let order: Vec<String> = items.into_iter().map(|(text, _)| text).collect();
        assert_eq!(order, strings(&case["expected"]), "{}", case["comment"]);
    }
}

#[test]
fn clip_keeps_whole_characters_and_marks_the_cut() {
    assert_eq!(clip("short", 10), "short");
    assert_eq!(clip("abcdefghij", 5), "abcd…");
    assert_eq!(clip("ééééé", 3), "éé…");
    assert_eq!(clip("  padded\nline  ", 20), "padded line");
}

#[test]
fn search_request_and_reply_have_a_stable_wire_shape() {
    let request: ClientRequest = serde_json::from_value(json!({
        "type": "SEARCH",
        "query": "oom",
        "scopes": ["COMMANDS", "EVENTS"],
        "filters": { "since": 1000, "outcome": "FAILED" },
        "limit": 20
    }))
    .unwrap();
    assert_eq!(
        request,
        ClientRequest::Search {
            query: "oom".into(),
            scopes: vec![SearchScope::Commands, SearchScope::Events],
            filters: SearchFilters {
                since: Some(Timestamp(1000)),
                outcome: Some(SearchOutcome::Failed),
            },
            limit: 20,
        }
    );
    // Scopes and filters may be left out: everything, unfiltered.
    let minimal: ClientRequest =
        serde_json::from_value(json!({ "type": "SEARCH", "query": "py", "limit": 5 })).unwrap();
    assert!(matches!(
        minimal,
        ClientRequest::Search { ref scopes, ref filters, .. }
            if scopes.is_empty() && *filters == SearchFilters::default()
    ));

    let reply = DaemonMessage::SearchResults {
        hits: vec![
            SearchHit {
                kind: SearchScope::Commands,
                title: "python train.py".into(),
                detail: "exit 1 · ~/ml".into(),
                target: SearchTarget::Command {
                    session_id: SessionId(3),
                    command_id: CommandId(9),
                },
                at: Some(Timestamp(5)),
                score: 95.0,
            },
            SearchHit {
                kind: SearchScope::Events,
                title: "Error".into(),
                detail: "CUDA out of memory".into(),
                target: SearchTarget::Event {
                    session_id: SessionId(3),
                    event_id: EventId(12),
                },
                at: Some(Timestamp(6)),
                score: 35.0,
            },
            SearchHit {
                kind: SearchScope::Recipes,
                title: "Eval".into(),
                detail: "recipe".into(),
                target: SearchTarget::Recipe {
                    recipe_id: "eval".into(),
                },
                at: None,
                score: 80.0,
            },
        ],
    };
    let value = serde_json::to_value(&reply).unwrap();
    assert_eq!(value["type"], "SEARCH_RESULTS");
    assert_eq!(
        value["hits"][0]["target"],
        json!({ "type": "COMMAND", "session_id": 3, "command_id": 9 })
    );
    assert_eq!(
        value["hits"][2]["target"],
        json!({ "type": "RECIPE", "recipe_id": "eval" })
    );
    assert_eq!(value["hits"][2].get("at"), Some(&Value::Null));
}

#[test]
fn search_requests_are_bounded() {
    let request = |query: String, limit: u32| ClientRequest::Search {
        query,
        scopes: Vec::new(),
        filters: SearchFilters::default(),
        limit,
    };
    assert!(request("oom".into(), 20).validate().is_ok());
    assert!(matches!(
        request("x".repeat(MAX_SEARCH_QUERY_BYTES + 1), 20).validate(),
        Err(ProtocolError::FieldTooLong { field: "query", .. })
    ));
    assert!(matches!(
        request("oom".into(), 0).validate(),
        Err(ProtocolError::InvalidQueryLimit {
            field: "search",
            ..
        })
    ));
    assert!(matches!(
        request("oom".into(), MAX_SEARCH_LIMIT + 1).validate(),
        Err(ProtocolError::InvalidQueryLimit {
            field: "search",
            ..
        })
    ));
}
