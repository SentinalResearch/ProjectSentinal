//! Milestone 5.A: completion contract and its frecency formula.

#![allow(clippy::expect_used, clippy::unwrap_used)]

use protocol::{
    complete::{
        completion_score, frecency, CandidateStats, Completion, CompletionSource,
        CompletionWeights, MAX_COMPLETE_LIMIT, MAX_COMPLETE_PREFIX_BYTES,
    },
    ClientRequest, DaemonMessage, LocalRequest, LocalResponse, ProtocolError, SessionId, Timestamp,
};
use serde_json::json;

const DAY: i64 = 86_400_000;
const NOW: Timestamp = Timestamp(100 * DAY);

fn stats(ok: u32, failed: u32, days_ago: i64) -> CandidateStats {
    CandidateStats {
        succeeded: ok,
        failed,
        last_at: Timestamp(NOW.0 - days_ago * DAY),
        in_project: 0,
        in_cwd: 0,
    }
}

#[test]
fn frecency_is_count_times_a_seven_day_half_life() {
    let weights = CompletionWeights::default();
    assert_eq!(frecency(&weights, 8, 0), 8.0);
    assert_eq!(frecency(&weights, 8, 7 * DAY), 4.0);
    assert_eq!(frecency(&weights, 8, 14 * DAY), 2.0);
    // A clock that went backwards is "just now", never more than the count.
    assert_eq!(frecency(&weights, 8, -DAY), 8.0);
}

#[test]
fn the_formula_adds_bonuses_and_a_failure_penalty() {
    let weights = CompletionWeights::default();
    // 4 successes today: frecency 4 + success bonus 2 × 4/4.
    assert_eq!(completion_score(&weights, &stats(4, 0, 0), NOW, false), 6.0);
    // Half the runs in this project, all in this cwd.
    let local = CandidateStats {
        in_project: 2,
        in_cwd: 4,
        ..stats(4, 0, 0)
    };
    assert_eq!(
        completion_score(&weights, &local, NOW, false),
        6.0 + 5.0 + 5.0
    );
    // Failures are left out of the count by default.
    assert_eq!(
        completion_score(&weights, &stats(3, 1, 0), NOW, false),
        3.0 + 1.5
    );
    // When asked for, they count, and the failed share is penalised.
    assert_eq!(
        completion_score(&weights, &stats(3, 1, 0), NOW, true),
        4.0 + 1.5 - 2.5
    );
}

#[test]
fn never_successful_commands_are_excluded_by_default() {
    let weights = CompletionWeights::default();
    assert_eq!(completion_score(&weights, &stats(0, 5, 0), NOW, false), 0.0);
    assert!(completion_score(&weights, &stats(0, 5, 0), NOW, true) > 0.0);
    // A Command still running has neither outcome yet.
    assert_eq!(completion_score(&weights, &stats(0, 0, 0), NOW, false), 0.0);
}

#[test]
fn the_spec_example_ranks_in_order() {
    // `python tr`: python train.py 31×, then with --config 6×, then the recipe.
    let weights = CompletionWeights::default();
    let plain = completion_score(&weights, &stats(31, 0, 1), NOW, false);
    let config = completion_score(&weights, &stats(6, 0, 0), NOW, false);
    assert!(plain > config, "{plain} > {config}");
    assert!(config > weights.recipe, "{config} > {}", weights.recipe);
}

#[test]
fn complete_has_a_stable_wire_shape_on_both_sockets() {
    let request: ClientRequest = serde_json::from_value(json!({
        "type": "COMPLETE",
        "session_id": 4,
        "prefix": "python tr",
        "cwd": "/home/me/ml",
        "limit": 5
    }))
    .unwrap();
    assert_eq!(
        request,
        ClientRequest::Complete {
            session_id: Some(SessionId(4)),
            prefix: "python tr".into(),
            cwd: Some("/home/me/ml".into()),
            include_failed: false,
            limit: 5,
        }
    );
    let local: LocalRequest = serde_json::from_value(json!({
        "type": "COMPLETE",
        "prefix": "git",
        "cwd": "/repo",
        "include_failed": true,
        "limit": 3
    }))
    .unwrap();
    assert!(matches!(
        local,
        LocalRequest::Complete {
            session_id: None,
            include_failed: true,
            ..
        }
    ));

    let completion = Completion {
        text: "python train.py".into(),
        source: CompletionSource::History,
        count: 31,
        last_at: Some(Timestamp(9)),
        last_exit: Some(0),
        score: 33.1,
    };
    let reply = serde_json::to_value(DaemonMessage::Completions {
        completions: vec![completion.clone()],
    })
    .unwrap();
    assert_eq!(reply["type"], "COMPLETIONS");
    assert_eq!(reply["completions"][0]["source"], "HISTORY");
    let local_reply = serde_json::to_value(LocalResponse::Completions {
        completions: vec![completion],
    })
    .unwrap();
    assert_eq!(local_reply["type"], "COMPLETIONS");
}

#[test]
fn complete_requests_are_bounded() {
    let request = |prefix: String, limit: u32| ClientRequest::Complete {
        session_id: None,
        prefix,
        cwd: None,
        include_failed: false,
        limit,
    };
    assert!(request("py".into(), 5).validate().is_ok());
    assert!(request(String::new(), 5).validate().is_ok());
    assert!(matches!(
        request("x".repeat(MAX_COMPLETE_PREFIX_BYTES + 1), 5).validate(),
        Err(ProtocolError::FieldTooLong {
            field: "prefix",
            ..
        })
    ));
    assert!(matches!(
        request("py".into(), MAX_COMPLETE_LIMIT + 1).validate(),
        Err(ProtocolError::InvalidQueryLimit {
            field: "complete",
            ..
        })
    ));
    let local = LocalRequest::Complete {
        session_id: None,
        prefix: "py".into(),
        cwd: Some("x".repeat(5_000)),
        include_failed: false,
        limit: 0,
    };
    assert!(local.validate().is_err());
}
