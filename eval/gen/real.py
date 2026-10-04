"""Real captured logs from crates/watcher/tests/logs, labelled by hand."""

from pathlib import Path

LOGS = Path(__file__).resolve().parents[2] / "crates/watcher/tests/logs"

P = lambda current, total, unit: {"name": "progress_updated", "args": {"current": current, "total": total, "unit": unit}}  # noqa: E731
M = lambda name, value: {"name": "metric_reported", "args": {"name": name, "value": value}}  # noqa: E731

LABELS = [
    ("training/hf_trainer.log", "accelerate launch train.py", "training", [
        P(2, 2, "epoch"), M("loss", 0.702), M("learning_rate", 5e-05), M("val_loss", 0.650),
        {"name": "checkpoint_created", "args": {"path": "output/checkpoint-500/model.safetensors"}},
    ]),
    ("training/lightning.log", "python train.py", "training", [
        P(3, 3, "epoch"), M("loss", 0.501), M("val_loss", 0.490), M("learning_rate", 0.00008),
        {"name": "checkpoint_created", "args": {"path": "checkpoints/epoch=2-val_loss=0.552.ckpt"}},
    ]),
    ("training/tqdm.log", "torchrun train_model.py", "training", [
        P(12, 100, "it"), M("loss", 1.0),
        {"name": "checkpoint_created", "args": {"path": "model-12.pt"}},
    ]),
    ("pytest/fail.log", "python -m pytest tests/", "tests", [
        {"name": "tests_summary", "args": {"passed": 2, "failed": 1, "skipped": 0}},
    ]),
    ("pytest/pass.log", "pytest tests/", "tests", [
        {"name": "tests_summary", "args": {"passed": 3, "failed": 0, "skipped": 0}},
    ]),
    ("cargo/build_error.log", "cargo build", "errors", [
        {"name": "error_detected", "args": {"kind": "build_error"}},
    ]),
    ("server/llama.log", "llama-server -m model.gguf", "servers", [
        {"name": "server_started", "args": {"port": 8080, "url": "http://127.0.0.1:8080"}},
    ]),
    ("server/vllm.log", "vllm serve Qwen/Qwen3-8B", "servers", [
        {"name": "server_started", "args": {"port": 8000, "url": "http://0.0.0.0:8000"}},
    ]),
]


OPTIONAL = {
    "training/hf_trainer.log": [P(0.5, None, "epoch")],
    "pytest/fail.log": [P(3, 3, "test"), {"name": "error_detected", "args": {"kind": "test_failure"}}],
    "pytest/pass.log": [P(3, 3, "test")],
}


def generate():
    for path, command, category, expected in LABELS:
        text = (LOGS / path).read_text(encoding="utf-8").rstrip("\n")
        yield {
            "id": "real-" + path.replace("/", "-").removesuffix(".log"),
            "suite": "translate",
            "category": category,
            "family": "real",
            "heldout": False,
            "edges": ["real_log", "raw_carriage_return"] if "\r" in text else ["real_log"],
            "input": {"command": command, "source": "log", "screen": text},
            "expected": expected,
            "forbidden": [],
            "optional": OPTIONAL.get(path, []),
        }
