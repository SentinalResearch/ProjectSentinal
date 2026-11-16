#![allow(clippy::unwrap_used)]
//! Terminal text is any UTF-8. No line may panic an adapter: the watcher runs
//! inside the PTY reader, and a panic there stops the Session being read.

use protocol::{Command, CommandId, MachineId, SessionId, Timestamp};
use watcher::{EventEngine, Observation};

const LINES: &[&str] = &[
    "🚀 loss: 0.5",
    "é loss=0.25",
    "日本 val_loss: 0.4 loss: 0.3",
    "🔥🔥 step 3/10 loss 1.0",
    "✓ accuracy: 0.9 🚀",
    "45%|████▌     | 450/1000 [00:10<00:12] 🚀",
    "Epoch 1/3 ━━━━━━━━ 120/500 loss: 0.7 ✨",
    "→ listening on 127.0.0.1:8080 🌐",
    "PASSED ✅ tests/test_é.py::test_ü",
    "⏺ Bash(echo 🚀)",
    "::sentinal progress current=1 total=2 unit=🚀",
    "::sentinal metric name=🚀 value=1",
    "Do you want to proceed? 🤔",
    "❯ 1. Yes ✅",
];

#[test]
fn no_adapter_panics_on_multibyte_text() {
    for command in [
        "python train.py",
        "pytest",
        "cargo test",
        "vllm serve model",
        "claude",
        "make",
    ] {
        let command = Command::start(
            CommandId(1),
            SessionId(1),
            MachineId("m".into()),
            command,
            "/w",
            Timestamp(0),
        );
        let mut engine = EventEngine::new(Some(&command), Vec::new());
        for (index, line) in LINES.iter().enumerate() {
            let at = Timestamp(i64::try_from(index).unwrap() * 100);
            engine.observe(&Observation::output(at, line));
            // Every suffix too, so a key lands right after a multi-byte char.
            for (start, _) in line.char_indices() {
                engine.observe(&Observation::output(at, &line[start..]));
            }
        }
        engine.observe(&Observation::screen(
            Timestamp(10_000),
            LINES.iter().map(|line| (*line).to_string()).collect(),
            false,
        ));
    }
}
