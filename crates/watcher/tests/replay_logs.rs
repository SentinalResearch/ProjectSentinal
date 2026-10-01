#![allow(clippy::unwrap_used)]
//! Saved real-world log replay snapshots.

use protocol::{Command, CommandId, MachineId, SessionId, Timestamp};
use watcher::{EventEngine, Observation};

fn command(text: &str) -> Command {
    Command::start(
        CommandId(1),
        SessionId(1),
        MachineId("machine".into()),
        text,
        "/work",
        Timestamp(0),
    )
}

fn replay(path: &str, command_text: &str) -> Vec<protocol::EventKind> {
    let text = std::fs::read_to_string(path).unwrap();
    let mut engine = EventEngine::new(Some(&command(command_text)), Vec::new());
    text.split_inclusive(['\n', '\r'])
        .enumerate()
        .flat_map(|(index, line)| {
            let clean = line.trim_end_matches(['\n', '\r']);
            engine
                .observe(&Observation::output(
                    Timestamp(i64::try_from(index).unwrap() * 1_000),
                    clean,
                ))
                .events
        })
        .collect()
}

#[test]
fn training_logs_produce_structured_events() {
    insta::assert_debug_snapshot!(
        "training_lightning",
        replay("tests/logs/training/lightning.log", "python train.py")
    );
    insta::assert_debug_snapshot!(
        "training_hf",
        replay(
            "tests/logs/training/hf_trainer.log",
            "accelerate launch train.py"
        )
    );
    insta::assert_debug_snapshot!(
        "training_tqdm",
        replay("tests/logs/training/tqdm.log", "torchrun train_model.py")
    );
}

#[test]
fn pytest_logs_produce_structured_events() {
    insta::assert_debug_snapshot!(
        "pytest_pass",
        replay("tests/logs/pytest/pass.log", "pytest tests/")
    );
    insta::assert_debug_snapshot!(
        "pytest_fail",
        replay("tests/logs/pytest/fail.log", "python -m pytest tests/")
    );
}

#[test]
fn server_logs_produce_structured_events() {
    insta::assert_debug_snapshot!(
        "vllm",
        replay("tests/logs/server/vllm.log", "vllm serve Qwen/Qwen3-8B")
    );
    insta::assert_debug_snapshot!(
        "llama_server",
        replay("tests/logs/server/llama.log", "llama-server -m model.gguf")
    );
}

#[test]
fn cargo_log_produces_structured_events() {
    insta::assert_debug_snapshot!(
        "cargo_error",
        replay("tests/logs/cargo/build_error.log", "cargo build")
    );
}
