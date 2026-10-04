"""Training runs in many log formats, with distractors, warnings, and crashes."""

from .common import (
    Line, checkpoint, error, example, metric, print_float, print_rate, progress, secret_line, warning,
)

# Families the test split keeps away from training data (unseen formats).
HELDOUT = {"wandb", "rl", "non_english", "lightning_bar"}

COMMANDS = [
    "python train.py --config configs/qwen.yaml",
    "torchrun --nproc_per_node=2 train.py",
    "accelerate launch finetune.py",
    "python -m trainer.main",
    "bash scripts/run_sft.sh",
    "deepspeed --num_gpus 2 sft.py",
    "python fit.py",
    "uv run train_lora.py",
]

HEADERS = [
    "Using GPU 0: NVIDIA GeForce RTX 5060 Ti (16 GB)",
    "torch 2.4.0+cu124 | transformers 4.46.1",
    "Loaded 48,213 training examples from data/train.jsonl",
    "batch_size: 32  grad_accum: 4  seed: 42",
    "model: Qwen/Qwen3-0.6B  max_seq_len: 2048",
    "***** Running training *****",
    "Trainable params: 4,194,304 || all params: 600,000,000 || trainable%: 0.70",
    "wandb: Tracking run with wandb version 0.18.3",
]

WARNINGS = [
    "UserWarning: torch.utils.checkpoint: the use_reentrant parameter should be passed explicitly.",
    "FutureWarning: `evaluation_strategy` is deprecated, use `eval_strategy` instead",
    "WARNING: gradient overflow, skipping step, reducing loss scale",
    "[W] DataLoader worker is slow; consider increasing num_workers",
]

CHECKPOINT_PHRASES = [
    "Saving checkpoint to {p}",
    "Saving model checkpoint to {p}",
    "Checkpoint saved: {p}",
    "saved weights at {p}",
    "Model weights saved in {p}",
    "INFO wrote checkpoint {p}",
]

CRASHES = [
    ("oom", [
        "Traceback (most recent call last):",
        '  File "train.py", line 212, in <module>',
        "torch.OutOfMemoryError: CUDA out of memory. Tried to allocate 2.00 GiB. GPU 0 has a total capacity of 15.93 GiB",
    ]),
    ("exception", [
        "Traceback (most recent call last):",
        '  File "train.py", line 88, in collate',
        "KeyError: 'labels'",
    ]),
    ("nan", ["Loss is NaN at step {s}, stopping training"]),
    ("killed", ["Killed"]),
]


NAME_SETS = [
    [("loss", None), ("lr", "rate")],
    [("loss", None), ("val_loss", None)],
    [("train_loss", None), ("acc", "acc")],
    [("loss", None), ("ppl", "ppl"), ("tok/s", "tps")],
    [("loss", None), ("grad_norm", "gn"), ("learning_rate", "rate")],
    [("nll", None), ("accuracy", "acc")],
]


def _metrics(rng, family, fraction, names):
    """Metric values for a point in training, under the run's fixed metric names."""
    base = 2.2 * (1 - fraction) + 0.25 + rng.uniform(-0.03, 0.03)
    if family == "rl":
        return [("reward", f"{5 + 20 * fraction + rng.uniform(-1, 1):.2f}"), ("kl", f"{0.01 + 0.04 * fraction:.4f}"), ("entropy", f"{1.4 - fraction:.3f}")]
    if family == "non_english":
        return [("损失", print_float(rng, base, "f3"))]
    printed = []
    for name, kind in names:
        if kind == "rate":
            printed.append((name, print_rate(rng, 2e-4 * (1 - 0.5 * fraction))))
        elif kind == "acc":
            printed.append((name, f"{0.55 + 0.4 * fraction:.4f}"))
        elif kind == "ppl":
            printed.append((name, f"{2.718 ** base:.2f}"))
        elif kind == "tps":
            printed.append((name, f"{rng.randrange(3000, 9000):,}"))
        elif kind == "gn":
            printed.append((name, f"{rng.uniform(0.4, 2.0):.3f}"))
        else:
            value = base * (1.08 if "val" in name else 1)
            printed.append((name, print_float(rng, value)))
    return printed


def _render(rng, family, unit, current, total, printed, epoch_total):
    """One progress line in `family`'s format and its facts."""
    facts = [metric(name, value) for name, value in printed]
    pairs = lambda sep, join: join.join(f"{name}{sep}{value}" for name, value in printed)  # noqa: E731
    if family == "plain":
        sep, join = rng.choice([(" ", "  "), ("=", " "), (": ", " | "), (": ", " - ")])
        word = rng.choice([unit.capitalize(), unit])
        return Line(f"{word} {current}/{total} {pairs(sep, join)}", [progress(current, total, unit)] + facts)
    if family == "of_words":
        return Line(f"{unit} {current} of {total}: {pairs(' ', ', ')}", [progress(current, total, unit)] + facts)
    if family == "logfmt":
        return Line(f"level=info msg=train {unit}={current}/{total} {pairs('=', ' ')}", [progress(current, total, unit)] + facts)
    if family == "jsonl":
        body = ", ".join(f'"{name}": {value.replace(",", "")}' for name, value in printed)
        facts = [metric(name, value.replace(",", "")) for name, value in printed]
        return Line(f'{{"{unit}": {current}, "total_{unit}s": {total}, {body}}}', [progress(current, total, unit)] + facts)
    if family == "hf_dict":
        body = ", ".join(f"'{name}': {value.replace(',', '')}" for name, value in printed)
        facts = [metric(name, value.replace(",", "")) for name, value in printed]
        bar = f"{int(current / total * 100):3d}%|{'█' * int(current / total * 10):<10}| {current}/{total} [01:02<00:41, 7.31it/s]"
        epoch = f"{current / total * epoch_total:.2f}"
        # A fractional epoch is honest progress, but not required: optional either way.
        return Line(f"{{{body}, 'epoch': {epoch}}}\n{bar}", [progress(current, total, "step"), ("optional_progress", float(epoch), None, "epoch")] + facts)
    if family == "tqdm":
        postfix = pairs("=", ", ")
        return Line(f"{int(current / total * 100):3d}%|{'█' * int(current / total * 10):<10}| {current}/{total} [00:42<00:21, 11.8it/s, {postfix}]", [progress(current, total, "it")] + facts)
    if family == "keras":
        body = " - ".join(f"{name}: {value}" for name, value in printed)
        return Line(f"{current * 10}/{total * 10} [==============>...............] - ETA: 12s - {body}", [progress(current * 10, total * 10, "step")] + facts)
    if family == "timestamped":
        return Line(f"2026-10-04 11:{rng.randrange(10, 59)}:{rng.randrange(10, 59)},{rng.randrange(100, 999)} - INFO - {unit.capitalize()} {current}/{total} - {pairs(': ', ' - ')}", [progress(current, total, unit)] + facts)
    if family == "rank_prefixed":
        return Line(f"[rank0]: {unit} {current}/{total} {pairs(' ', ' ')}", [progress(current, total, unit)] + facts)
    if family == "percent_only":
        percent = int(current / total * 100)
        return Line(f"Training... {percent}% complete ({pairs('=', ', ')})", [progress(percent, 100, "%")] + facts)
    if family == "wandb":
        logged = [("train/loss" if name in ("loss", "train_loss") else name, value) for name, value in printed]
        body = " | ".join(f"{name}: {value}" for name, value in logged)
        return Line(f"{body} | global_step: {current}", [progress(current, None, "step")] + [metric(name, value) for name, value in logged])
    if family == "rl":
        return Line(f"iter {current}/{total} | {pairs(' ', ' | ')}", [progress(current, total, "iter")] + facts)
    if family == "non_english":
        return Line(f"第 {current}/{total} 轮  {pairs('=', '  ')}", [progress(current, total, "epoch")] + facts)
    if family == "lightning_bar":
        postfix = pairs("=", ", ")
        return Line(f"Epoch {current}: {int(current / total * 100)}%|████▌     | {current * 20}/{total * 20} [00:30<00:37, 2.95it/s, v_num=3, {postfix}]", [progress(current, None, "epoch"), progress(current * 20, total * 20, "it")] + facts)
    raise ValueError(family)


FAMILIES = ["plain", "of_words", "logfmt", "jsonl", "hf_dict", "tqdm", "keras", "timestamped", "rank_prefixed", "percent_only", "wandb", "rl", "non_english", "lightning_bar"]


def generate(rng, index, family):
    unit = rng.choice(["epoch", "step"]) if family in {"plain", "of_words", "logfmt", "jsonl", "timestamped", "rank_prefixed"} else "epoch"
    total = rng.choice([3, 5, 10, 20, 30, 100]) if unit == "epoch" else rng.choice([500, 1000, 2000, 10000])
    if family in {"keras", "lightning_bar"}:
        total = rng.choice([5, 10, 20])
    current_end = rng.randrange(1, total + 1)
    step_size = max(1, total // rng.choice([5, 10, 20]))
    edges = [family]
    lines = [Line(text) for text in rng.sample(HEADERS, rng.randrange(1, 4))]
    if any(char.isdigit() for line in lines for char in line.text):
        edges.append("distractor_numbers")
    if rng.random() < 0.15:
        lines.append(secret_line(rng))
        edges.append("secret")
    if family == "keras":
        lines.append(Line(f"Epoch {max(1, current_end // 10)}/{max(1, total // 10)}", [progress(max(1, current_end // 10), max(1, total // 10), "epoch")]))
    epoch_total = rng.choice([2, 3])
    checkpoint_at = rng.randrange(1, total + 1) if rng.random() < 0.4 else None
    names = rng.choice(NAME_SETS)
    for current in range(step_size, current_end + 1, step_size):
        printed = _metrics(rng, family, current / total, names)
        lines.append(_render(rng, family, unit, current, total, printed, epoch_total))
        if checkpoint_at and current >= checkpoint_at and "checkpoint" not in edges:
            path = rng.choice(["checkpoints/epoch_{c}.pt", "output/checkpoint-{c}", "ckpt/step-{c}/model.safetensors", "models/run-7/epoch={c}-val_loss=0.55.ckpt"]).format(c=current)
            lines.append(Line(rng.choice(CHECKPOINT_PHRASES).format(p=path), [checkpoint(path)]))
            edges.append("checkpoint")
        if rng.random() < 0.08:
            lines.append(Line(rng.choice(WARNINGS), [warning()]))
            edges.append("warning")
    if rng.random() < 0.1:
        path = f"checkpoints/epoch_{rng.randrange(1, 9)}.pt"
        lines.append(Line(f"Loading checkpoint from {path}"))
        edges.append("checkpoint_load_distractor")
    if rng.random() < 0.12:
        kind, crash = rng.choice(CRASHES)
        lines += [Line(text.format(s=current_end), [error(kind)] if text == crash[-1] else []) for text in crash]
        edges.append(f"crash_{kind}")
    command = rng.choice(COMMANDS)
    if command.startswith(("bash", "uv", "deepspeed")) or "finetune" in command or "fit.py" in command:
        edges.append("unusual_launcher")
    return example(f"train-{family}-{index:04d}", "training", family, edges, command, lines, heldout=family in HELDOUT)
