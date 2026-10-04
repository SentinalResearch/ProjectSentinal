"""Shared vocabulary for the generator and the scorer.

A model may say `lr` or `learning_rate`, `it` or `step`; both mean the same
thing to Sentinal, so names are compared after canonicalisation.
"""

import re

METRIC_SYNONYMS = {
    "loss": ["loss", "train_loss", "train/loss", "training_loss", "train loss", "training loss", "损失"],
    "val_loss": ["val_loss", "eval_loss", "valid_loss", "validation_loss", "val/loss", "eval/loss", "val loss", "validation loss"],
    "learning_rate": ["learning_rate", "lr", "train/lr", "train/learning_rate", "learning rate"],
    "accuracy": ["accuracy", "acc", "train_acc", "train_accuracy", "train/acc"],
    "val_accuracy": ["val_accuracy", "val_acc", "eval_accuracy", "eval_acc", "val/acc"],
    "perplexity": ["perplexity", "ppl", "eval_ppl"],
    "grad_norm": ["grad_norm", "gradient_norm", "gnorm"],
    "reward": ["reward", "mean_reward", "rollout/ep_rew_mean", "ep_rew_mean"],
    "kl": ["kl", "kl_div", "approx_kl"],
    "entropy": ["entropy", "policy_entropy"],
    "tokens_per_second": ["tokens_per_second", "tok/s", "tokens/s", "tokens_per_sec", "tps", "throughput"],
    "samples_per_second": ["samples_per_second", "samples/s", "samples/sec", "train_samples_per_second"],
    "f1": ["f1", "f1_score", "eval_f1"],
    "bleu": ["bleu", "eval_bleu"],
    "nll": ["nll", "neg_log_likelihood"],
}

_METRIC_LOOKUP = {alias.lower(): name for name, aliases in METRIC_SYNONYMS.items() for alias in aliases}

UNIT_GROUPS = {
    "epoch": ["epoch", "epochs", "ep", "轮"],
    "step": ["step", "steps", "it", "its", "iter", "iters", "iteration", "iterations", "batch", "batches", "update", "updates"],
    "%": ["%", "percent", "pct"],
    "test": ["test", "tests"],
    "file": ["file", "files", "shard", "shards"],
    "object": ["object", "objects"],
    "size": ["b", "kb", "mb", "gb", "kib", "mib", "gib", "k", "m", "g", "byte", "bytes"],
}

_UNIT_LOOKUP = {alias: group for group, aliases in UNIT_GROUPS.items() for alias in aliases}


def metric_name(name: str) -> str:
    """Canonical metric name; unknown names are lower-cased and kept."""
    key = name.strip().lower()
    return _METRIC_LOOKUP.get(key, key.replace(" ", "_"))


def unit_group(unit: str) -> str:
    key = unit.strip().lower()
    return _UNIT_LOOKUP.get(key, key)


_NUMBER = re.compile(r"[-+]?(?:\d{1,3}(?:,\d{3})+|\d+)?(?:\.\d+)?(?:[eE][-+]?\d+)?")


def numbers_in(text: str) -> list:
    """Every number written in `text`, thousands separators allowed."""
    found = []
    for match in _NUMBER.finditer(text):
        token = match.group(0)
        if not any(character.isdigit() for character in token):
            continue
        try:
            found.append(float(token.replace(",", "")))
        except ValueError:
            continue
    return found


def close(a: float, b: float) -> bool:
    """Equal within printing precision."""
    return abs(a - b) <= max(1e-9, 1e-3 * max(abs(a), abs(b)))


def normalize_template(template: str) -> str:
    """Recipe templates compare with placeholder names erased."""
    return re.sub(r"\s+", " ", re.sub(r"\{[^{}]*\}", "{}", template)).strip()
