#!/usr/bin/env python3
"""Generate the Sentinal translator evaluation dataset (standard library only).

    python3 eval/generate.py                 # test + dev into eval/data/
    python3 eval/generate.py --train 5000    # also a fine-tuning split (not committed)

Every example carries its exact expected calls. The same seed always produces
the same files, so models are compared on identical data.
"""

import argparse
import hashlib
import json
import random
import sys
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT))

from gen import events, negatives, real, recipes, training  # noqa: E402
from gen.common import make_rng  # noqa: E402

DATASET_VERSION = 1

# Examples per category at scale 1.0 (the test split).
QUOTAS = {
    "training": 616,  # 44 per format family
    "errors": 160,
    "prompts": 120,
    "servers": 120,
    "tests": 120,
    "downloads": 80,
    "negatives": 160,
    "recipes": 200,
}

GENERATORS = {
    "errors": events.errors,
    "prompts": events.prompts,
    "servers": events.servers,
    "tests": events.test_runs,
    "downloads": events.downloads,
    "negatives": negatives.generate,
    "recipes": recipes.generate,
}


def _as_raw_log(rng, record):
    """Some inputs are tailed log files: colour codes survive, nothing is rendered."""
    lines = record["input"]["screen"].split("\n")
    colours = ["\x1b[32m", "\x1b[1;34m", "\x1b[33m", "\x1b[2m"]
    record["input"]["screen"] = "\n".join(
        f"{rng.choice(colours)}{line}\x1b[0m" if line and rng.random() < 0.4 else line for line in lines
    )
    record["input"]["source"] = "log"
    record["edges"] = sorted(set(record["edges"] + ["ansi_codes"]))
    return record


def build(split, seed, scale, include_heldout):
    records = []
    families = [family for family in training.FAMILIES if include_heldout or family not in training.HELDOUT]
    per_family = max(1, round(QUOTAS["training"] * scale / len(training.FAMILIES)))
    for family in families:
        for index in range(per_family):
            rng = make_rng(seed, split, "training", family, index)
            record = training.generate(rng, index, family)
            records.append(_as_raw_log(rng, record) if rng.random() < 0.15 else record)
    for category, generator in GENERATORS.items():
        for index in range(max(1, round(QUOTAS[category] * scale))):
            rng = make_rng(seed, split, category, index)
            record = generator(rng, index)
            if category in {"errors", "servers"} and rng.random() < 0.15:
                record = _as_raw_log(rng, record)
            records.append(record)
    if split != "train":
        records.extend(real.generate())
    unique = {}
    for record in records:
        unique.setdefault(_input_key(record), record)
    records = list(unique.values())
    for record in records:
        record["id"] = f"{split}-{record['id']}"
        record["split"] = split
    random.Random(f"{seed}:{split}:order").shuffle(records)
    return records


def _input_key(record):
    return hashlib.sha256(json.dumps(record["input"], sort_keys=True).encode()).hexdigest()


def write(path, records):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for record in records:
            handle.write(json.dumps(record, ensure_ascii=False) + "\n")


def summary(records):
    return {
        "examples": len(records),
        "expected_calls": sum(len(record["expected"]) for record in records),
        "by_category": dict(sorted(Counter(record["category"] for record in records).items())),
        "heldout_examples": sum(record["heldout"] for record in records),
        "with_secrets": sum(bool(record["forbidden"]) for record in records),
        "nothing_expected": sum(not record["expected"] for record in records),
        "by_edge": dict(sorted(Counter(edge for record in records for edge in record["edges"]).items())),
        "by_function": dict(sorted(Counter(call["name"] for record in records for call in record["expected"]).items())),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--out", type=Path, default=ROOT / "data")
    parser.add_argument("--seed", default="sentinal-eval-v1")
    parser.add_argument("--train", type=int, default=0, help="also write a training split of about this many examples")
    args = parser.parse_args()

    test = build("test", args.seed, 1.0, include_heldout=True)
    dev = build("dev", args.seed, 0.2, include_heldout=False)
    write(args.out / "test.jsonl", test)
    write(args.out / "dev.jsonl", dev)
    manifest = {"dataset_version": DATASET_VERSION, "seed": args.seed, "test": summary(test), "dev": summary(dev)}
    if args.train:
        total = sum(QUOTAS.values())
        train = build("train", args.seed, args.train / total, include_heldout=False)
        held = {_input_key(record) for record in test}
        train = [record for record in train if _input_key(record) not in held]
        write(args.out / "train.jsonl", train)
        manifest["train"] = summary(train)
    (args.out / "manifest.json").write_text(json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    for split in ("test", "dev", "train"):
        if split in manifest:
            print(f"{split}: {manifest[split]['examples']} examples, {manifest[split]['expected_calls']} expected calls")


if __name__ == "__main__":
    main()
