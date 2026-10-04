//! Run today's deterministic adapters over the translator evaluation dataset.
//!
//! cargo run -q -p watcher --example eval_baseline -- eval/data/test.jsonl > baseline.jsonl
//!
//! Output is the prediction format `eval/score.py` reads. Adapters do not
//! classify errors or prompts, so errors map to `other` and prompts get a
//! keyword guess; Recipes are outside the adapters' scope (no calls).

use protocol::{Command, CommandId, EventKind, MachineId, SessionId, Timestamp};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    time::Instant,
};
use watcher::{AdapterState, EventEngine, Observation};

fn main() -> io::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "eval/data/test.jsonl".into());
    let file = std::fs::File::open(&path)?;
    let mut out = io::stdout().lock();
    for line in io::BufReader::new(file).lines() {
        let record: Value = serde_json::from_str(&line?).map_err(io::Error::other)?;
        let id = record["id"].as_str().unwrap_or_default().to_owned();
        let started = Instant::now();
        let calls = if record["suite"] == "translate" {
            translate(&record["input"])
        } else {
            Vec::new()
        };
        let latency = started.elapsed().as_secs_f64() * 1_000.0;
        writeln!(
            out,
            "{}",
            json!({"id": id, "calls": calls, "latency_ms": latency})
        )?;
    }
    Ok(())
}

fn translate(input: &Value) -> Vec<Value> {
    let command = Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("eval".into()),
        input["command"].as_str().unwrap_or_default(),
        "/work",
        Timestamp(0),
    );
    let mut engine = EventEngine::new(Some(&command), Vec::new());
    let screen = input["screen"].as_str().unwrap_or_default();
    let mut progress = BTreeMap::new();
    let mut checkpoints = Vec::new();
    let mut calls = Vec::new();
    let mut last_state = None;
    let mut prompt: Option<String> = None;
    for (index, line) in screen.split_inclusive(['\n', '\r']).enumerate() {
        let at = Timestamp(i64::try_from(index).unwrap_or(i64::MAX) * 1_000);
        let update = engine.observe(&Observation::output(
            at,
            line.trim_end_matches(['\n', '\r']),
        ));
        if update.adapter_state.is_some() {
            last_state = update.adapter_state;
        }
        for event in update.events {
            match event {
                EventKind::ProgressUpdated { current, total, unit } => {
                    progress.insert(unit.clone(), json!({"current": current, "total": total, "unit": unit}));
                }
                EventKind::CheckpointCreated { path } if !checkpoints.contains(&path) => checkpoints.push(path),
                EventKind::ErrorDetected { message } => calls.push(json!({"name": "error_detected", "args": {"kind": "other", "summary": message.chars().take(120).collect::<String>()}})),
                EventKind::WarningDetected { message } => calls.push(json!({"name": "warning_detected", "args": {"summary": message.chars().take(120).collect::<String>()}})),
                EventKind::ServerStarted { port, url } => calls.push(json!({"name": "server_started", "args": {"port": port, "url": url}})),
                EventKind::WaitingForInput { prompt: text } => prompt = Some(text.unwrap_or_default()),
                _ => {}
            }
        }
    }
    // A prompt only waits if nothing was printed after it.
    let last_line = screen
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    let waiting = engine.observe(&Observation {
        at: Timestamp(1_000_000),
        kind: watcher::ObservationKind::Tick,
    });
    for event in waiting.events {
        if let EventKind::WaitingForInput { prompt: text } = event {
            prompt = Some(text.unwrap_or_default());
        }
    }
    if prompt.is_some_and(|text| last_line.contains(text.trim()) || text.is_empty()) {
        let lower = last_line.to_lowercase();
        let kind = if lower.contains("password") || lower.contains("passphrase") {
            "password"
        } else if lower.contains("press") {
            "key"
        } else if ["y/n", "yes/no", "(y)", "[y/", "proceed", "continue?"]
            .iter()
            .any(|word| lower.contains(word))
        {
            "confirm"
        } else {
            "text"
        };
        calls.push(json!({"name": "waiting_for_input", "args": {"kind": kind, "prompt": last_line.trim()}}));
    }
    calls.extend(
        progress
            .into_values()
            .map(|args| json!({"name": "progress_updated", "args": args})),
    );
    calls.extend(
        checkpoints
            .into_iter()
            .map(|path| json!({"name": "checkpoint_created", "args": {"path": path}})),
    );
    match last_state {
        Some(AdapterState::Training(state)) => {
            for (name, value) in [
                ("loss", state.last_loss),
                ("val_loss", state.best_val_loss),
                ("learning_rate", state.learning_rate),
            ] {
                if let Some(value) = value {
                    calls.push(
                        json!({"name": "metric_reported", "args": {"name": name, "value": value}}),
                    );
                }
            }
        }
        Some(AdapterState::Pytest(state)) => {
            if let (Some(passed), Some(failed)) = (state.passed, state.failed) {
                calls.push(json!({"name": "tests_summary", "args": {"passed": passed, "failed": failed, "skipped": 0}}));
            }
        }
        Some(AdapterState::Cargo(state)) => {
            if let (Some(passed), Some(failed)) = (state.tests_passed, state.tests_failed) {
                calls.push(json!({"name": "tests_summary", "args": {"passed": passed, "failed": failed, "skipped": 0}}));
            }
        }
        _ => {}
    }
    calls
}
