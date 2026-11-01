//! Check an agent's proposed Events against the screen it was shown (6.2).
//!
//! The intelligence service already checks its model's reply; the control
//! checks again, because the agent is just another client. A proposal must
//! name a function from `eval/tools.json` (screen functions only), match its
//! argument types and enums, and be grounded: every number, path, URL and
//! prompt must be on the screen the control itself fetched. Unknown
//! arguments are dropped; anything secret-shaped is refused.

use protocol::agent::{AgentScreen, ProposedEvent};
use serde_json::{Map, Value};
use std::sync::OnceLock;

const TOOLS: &str = include_str!("../../../../eval/tools.json");
const SUMMARY_LIMIT: usize = 120;
/// The training answers leave these out, so a trained model omits them.
const NOT_REQUIRED: &[(&str, &str)] = &[("error_detected", "summary"), ("warning_detected", "summary")];

fn functions() -> &'static Map<String, Value> {
    static FUNCTIONS: OnceLock<Map<String, Value>> = OnceLock::new();
    FUNCTIONS.get_or_init(|| {
        let tools: Value = serde_json::from_str(TOOLS).unwrap_or(Value::Null);
        tools["functions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|function| Some((function["name"].as_str()?.to_string(), function["parameters"].clone())))
            // Recipes are proposed from history, never from a screen.
            .filter(|(name, _)| name != "propose_recipe")
            .collect()
    })
}

fn types(schema: &Value) -> Vec<&str> {
    match &schema["type"] {
        Value::String(kind) => vec![kind.as_str()],
        Value::Array(kinds) => kinds.iter().filter_map(Value::as_str).collect(),
        _ => vec!["string"],
    }
}

fn matches(value: &Value, kind: &str) -> bool {
    match kind {
        "null" => value.is_null(),
        "boolean" => value.is_boolean(),
        "integer" => value.as_i64().is_some() || value.as_f64().is_some_and(|n| n.fract() == 0.0),
        "number" => value.is_number(),
        "string" => value.is_string(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => false,
    }
}

fn conform(value: &Value, schema: &Value, name: &str) -> Result<Value, String> {
    if !types(schema).iter().any(|kind| matches(value, kind)) {
        return Err(format!("{name} has the wrong type"));
    }
    if let Some(allowed) = schema["enum"].as_array() {
        if !allowed.contains(value) {
            return Err(format!("{name} is not allowed"));
        }
    }
    Ok(value.clone())
}

fn numbers_in(text: &str) -> Vec<f64> {
    let mut found = Vec::new();
    let mut current = String::new();
    for character in text.chars().chain(std::iter::once(' ')) {
        if character.is_ascii_digit() || matches!(character, '.' | 'e' | 'E' | '-' | '+') && !current.is_empty() || character == '-' {
            current.push(character);
        } else if character == ',' && !current.is_empty() {
            continue;
        } else {
            let token = current.trim_end_matches(['.', 'e', 'E', '-', '+']);
            if token.chars().any(|c| c.is_ascii_digit()) {
                if let Ok(number) = token.parse::<f64>() {
                    found.push(number);
                }
            }
            current.clear();
        }
    }
    found
}

fn on_screen(number: f64, present: &[f64]) -> bool {
    number == 0.0
        || present
            .iter()
            .any(|seen| (number - seen).abs() <= f64::max(1e-9, 1e-3 * number.abs().max(seen.abs())))
}

fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| strings(item, out)),
        _ => {}
    }
}

/// The proposal as it may be kept, or a short reason it was refused.
pub fn check(proposal: &ProposedEvent, screen: &AgentScreen) -> Result<Value, String> {
    let name = proposal.function.as_str();
    let schema = functions().get(name).ok_or("unknown function")?;
    let args = proposal.args.as_object().ok_or("arguments are not an object")?;
    for required in schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if !args.contains_key(required) && !NOT_REQUIRED.contains(&(name, required)) {
            return Err(format!("{required} is missing"));
        }
    }
    let mut kept = Map::new();
    for (key, property) in schema["properties"].as_object().into_iter().flatten() {
        if let Some(value) = args.get(key) {
            kept.insert(key.clone(), conform(value, property, key)?);
        }
    }
    let text = screen.lines.join("\n");
    let present = numbers_in(&text);
    let numeric = match name {
        "progress_updated" => &["current", "total"][..],
        "metric_reported" => &["value"][..],
        "server_started" => &["port"][..],
        _ => &[][..],
    };
    for key in numeric {
        let Some(number) = kept.get(*key).and_then(Value::as_f64) else { continue };
        let percent = kept.get("unit").and_then(Value::as_str).is_some_and(|unit| unit == "%");
        if *key == "total" && percent && number == 100.0 {
            continue;
        }
        if !on_screen(number, &present) {
            return Err(format!("{key} is not on screen"));
        }
    }
    if name == "checkpoint_created" {
        let path = kept["path"].as_str().unwrap_or_default().trim().trim_matches(['\'', '"', '`']).to_string();
        if path.is_empty() || !text.contains(&path) {
            return Err("path is not on screen".into());
        }
        kept.insert("path".into(), Value::String(path));
    }
    if name == "server_started" {
        if let Some(url) = kept.get("url").and_then(Value::as_str) {
            let url = url.split(['?', '#']).next().unwrap_or_default().to_string();
            if !text.to_lowercase().contains(url.trim_end_matches('/').to_lowercase().as_str()) {
                return Err("url is not on screen".into());
            }
            kept.insert("url".into(), Value::String(url));
        }
    }
    if name == "waiting_for_input" {
        let last = screen.lines.iter().rev().find(|line| !line.trim().is_empty()).map(|line| line.to_lowercase());
        let prompt = kept["prompt"].as_str().unwrap_or_default().trim().to_lowercase();
        if prompt.is_empty() || !last.is_some_and(|line| line.contains(&prompt)) {
            return Err("prompt is not on the last line".into());
        }
    }
    let mut texts = Vec::new();
    strings(&Value::Object(kept.clone()), &mut texts);
    if texts.iter().any(|text| watcher::redact::redact(text).1 > 0 || text.contains(watcher::redact::REDACTED)) {
        return Err("contains a secret".into());
    }
    if kept.get("summary").and_then(Value::as_str).is_some_and(|summary| summary.chars().count() > SUMMARY_LIMIT) {
        return Err("summary is too long".into());
    }
    Ok(Value::Object(kept))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use protocol::{MachineId, SessionId};
    use serde_json::json;

    fn screen(lines: &[&str]) -> AgentScreen {
        AgentScreen {
            machine_id: MachineId("m".into()),
            session_id: SessionId(7),
            command: Some("python train.py".into()),
            lines: lines.iter().map(|line| line.to_string()).collect(),
            redacted: 0,
        }
    }

    fn proposal(function: &str, args: Value) -> ProposedEvent {
        ProposedEvent { function: function.into(), args }
    }

    #[test]
    fn grounded_proposals_pass_and_extra_arguments_are_dropped() {
        let screen = screen(&["Epoch 12/40 loss=0.81", "Saved runs/ckpt-12.pt"]);
        let kept = check(&proposal("progress_updated", json!({"current": 12, "total": 40, "unit": "epoch", "cmd": "rm"})), &screen).unwrap();
        assert_eq!(kept, json!({"current": 12, "total": 40, "unit": "epoch"}));
        assert!(check(&proposal("metric_reported", json!({"name": "loss", "value": 0.81})), &screen).is_ok());
        assert!(check(&proposal("checkpoint_created", json!({"path": "runs/ckpt-12.pt"})), &screen).is_ok());
        assert!(check(&proposal("warning_detected", json!({})), &screen).is_ok(), "summary is optional");
    }

    #[test]
    fn invented_unknown_and_secret_proposals_are_refused() {
        let screen = screen(&["Epoch 12/40 loss=0.81", "Open http://localhost:8888/lab?token=[redacted]", "Continue? [y/N]"]);
        let cases = [
            (proposal("progress_updated", json!({"current": 13, "total": 40, "unit": "epoch"})), "current is not on screen"),
            (proposal("metric_reported", json!({"name": "loss", "value": 0.1})), "value is not on screen"),
            (proposal("send_input", json!({"text": "y\r"})), "unknown function"),
            (proposal("propose_recipe", json!({"template": "x"})), "unknown function"),
            (proposal("checkpoint_created", json!({"path": "/etc/passwd"})), "path is not on screen"),
            (proposal("server_started", json!({"port": 8888, "url": "http://evil.example/"})), "url is not on screen"),
            (proposal("error_detected", json!({"kind": "meltdown"})), "kind is not allowed"),
            (proposal("tests_summary", json!({"passed": "many", "failed": 0, "skipped": 0})), "passed has the wrong type"),
            (proposal("warning_detected", json!({"summary": "key sk-abcdefghijklmnopqrstuvwx"})), "contains a secret"),
            (proposal("warning_detected", json!({"summary": "w".repeat(121)})), "summary is too long"),
            (proposal("waiting_for_input", json!({"kind": "confirm", "prompt": "Delete everything?"})), "prompt is not on the last line"),
        ];
        for (proposal, reason) in cases {
            assert_eq!(check(&proposal, &screen).unwrap_err(), reason, "{proposal:?}");
        }
        let url = check(&proposal("server_started", json!({"port": 8888, "url": "http://localhost:8888/lab?token=abc"})), &screen).unwrap();
        assert_eq!(url["url"], "http://localhost:8888/lab");
        assert!(check(&proposal("waiting_for_input", json!({"kind": "confirm", "prompt": "Continue? [y/N]"})), &screen).is_ok());
    }

    #[test]
    fn numbers_are_read_like_the_eval_scorer_reads_them() {
        assert_eq!(numbers_in("step 1,200/5,000 lr=3e-4 loss -0.5"), vec![1200.0, 5000.0, 3e-4, -0.5]);
        assert!(on_screen(0.00030001, &[3e-4]));
    }
}
