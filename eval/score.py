#!/usr/bin/env python3
"""Score translator predictions against the Sentinal evaluation dataset.

    python3 eval/score.py eval/data/test.jsonl predictions.jsonl [--json report.json] [--edges]

Predictions are JSON lines: {"id": ..., "calls": [{"name": ..., "args": {...}}],
"latency_ms": optional, "error": optional}. A missing id counts as no calls.
"""

import argparse
import json
import statistics
import sys
from collections import Counter, defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from canon import close, metric_name, normalize_template, numbers_in, unit_group  # noqa: E402

TOOLS = {function["name"]: function for function in json.loads((Path(__file__).resolve().parent / "tools.json").read_text())["functions"]}
ERROR_KINDS = set(TOOLS["error_detected"]["parameters"]["properties"]["kind"]["enum"])
PROMPT_KINDS = set(TOOLS["waiting_for_input"]["parameters"]["properties"]["kind"]["enum"])


def _number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def valid(call):
    """Name exists and required arguments have the right shape."""
    if not isinstance(call, dict) or call.get("name") not in TOOLS or not isinstance(call.get("args", {}), dict):
        return False
    name, args = call["name"], call.get("args", {})
    if name == "progress_updated":
        return _number(args.get("current")) and (args.get("total") is None or _number(args.get("total"))) and isinstance(args.get("unit"), str)
    if name == "metric_reported":
        return isinstance(args.get("name"), str) and _number(args.get("value"))
    if name == "checkpoint_created":
        return isinstance(args.get("path"), str) and bool(args["path"].strip())
    if name == "error_detected":
        return args.get("kind") in ERROR_KINDS
    if name == "waiting_for_input":
        return args.get("kind") in PROMPT_KINDS and isinstance(args.get("prompt"), str)
    if name == "server_started":
        return args.get("port") is None or isinstance(args.get("port"), int)
    if name == "tests_summary":
        return all(isinstance(args.get(key), int) for key in ("passed", "failed", "skipped"))
    if name == "propose_recipe":
        return isinstance(args.get("template"), str) and isinstance(args.get("params", []), list)
    return True


def _url(url):
    return url.split("?")[0].rstrip("/").lower() if isinstance(url, str) else None


def matches(expected, predicted):
    """Whether `predicted` satisfies `expected` (same function name assumed)."""
    want, got = expected["args"], predicted.get("args", {})
    name = expected["name"]
    if name == "progress_updated":
        if unit_group(got["unit"]) == unit_group(want["unit"]) and close(got["current"], want["current"]):
            return want["total"] is None or (got.get("total") is not None and close(got["total"], want["total"]))
        # A percentage reading of the same progress is equally correct.
        if unit_group(got["unit"]) == "%" and want["total"]:
            return abs(got["current"] - want["current"] / want["total"] * 100) <= 1.0
        return False
    if name == "metric_reported":
        return metric_name(got["name"]) == want["name"] and close(got["value"], want["value"])
    if name == "checkpoint_created":
        return got["path"].strip().strip("'\"`") == want["path"]
    if name == "error_detected":
        return got["kind"] == want["kind"]
    if name == "warning_detected":
        return True
    if name == "waiting_for_input":
        a, b = got["prompt"].strip().lower(), want["prompt"].strip().lower()
        return got["kind"] == want["kind"] and len(a) >= 3 and (a in b or b in a)
    if name == "server_started":
        if got.get("port") != want["port"]:
            return False
        return want["url"] is None or _url(got.get("url")) == _url(want["url"])
    if name == "tests_summary":
        return all(got[key] == want[key] for key in ("passed", "failed", "skipped"))
    if name == "propose_recipe":
        return normalize_template(got["template"]) == normalize_template(want["template"])
    return False


def _assign(expected, predicted):
    """Greedy one-to-one matching; returns matched pairs and leftovers."""
    unused = list(range(len(predicted)))
    pairs, missed = [], []
    for want in expected:
        hit = next((index for index in unused if predicted[index]["name"] == want["name"] and matches(want, predicted[index])), None)
        if hit is None:
            missed.append(want)
        else:
            unused.remove(hit)
            pairs.append((want, predicted[hit]))
    return pairs, missed, [predicted[index] for index in unused]


def _grounding(record, call):
    """Values in `call` that do not appear in the input (invented), and values checked."""
    if record["suite"] == "recipes":
        text = "\n".join(entry["command"] for entry in record["input"]["history"])
        values = [param.get("default") for param in call["args"].get("params", []) if param.get("default") is not None]
        return [value for value in values if str(value) not in text], len(values)
    text = record["input"]["screen"]
    present = numbers_in(text)
    args = call.get("args", {})
    checked, invented = 0, []
    # Test counts may be derived (passed = ran - failed - skipped), and a
    # percentage implies a total of 100; neither has to be printed.
    keys = [] if call["name"] == "tests_summary" else ["current", "total", "value", "port"]
    for key in keys:
        value = args.get(key)
        if key == "total" and unit_group(str(args.get("unit", ""))) == "%" and value == 100:
            continue
        if _number(value) and value != 0:
            checked += 1
            if not any(close(value, number) for number in present):
                invented.append(value)
    if call["name"] == "checkpoint_created":
        checked += 1
        if args["path"].strip().strip("'\"`") not in text:
            invented.append(args["path"])
    return invented, checked


class Tally:
    def __init__(self):
        self.tp = self.fp = self.fn = 0

    def add(self, tp, fp, fn):
        self.tp += tp
        self.fp += fp
        self.fn += fn

    def row(self):
        precision = self.tp / (self.tp + self.fp) if self.tp + self.fp else 1.0
        recall = self.tp / (self.tp + self.fn) if self.tp + self.fn else 1.0
        f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
        return {"precision": round(precision, 4), "recall": round(recall, 4), "f1": round(f1, 4), "tp": self.tp, "fp": self.fp, "fn": self.fn}


def score(dataset, predictions):
    overall, by_function, by_category, by_split, by_edge = Tally(), defaultdict(Tally), defaultdict(Tally), defaultdict(Tally), defaultdict(Tally)
    exact = Counter()
    invented_total = checked_total = 0
    leaks, invalid, missing, negatives_clean, negatives = 0, 0, 0, 0, 0
    defaults_right = defaults_total = 0
    errors_seen = errors_expected = 0
    latencies, failures = [], []
    for record in dataset:
        prediction = predictions.get(record["id"])
        if prediction is None:
            missing += 1
            prediction = {"calls": []}
        if isinstance(prediction.get("latency_ms"), (int, float)):
            latencies.append(prediction["latency_ms"])
        raw = prediction.get("calls") or []
        calls = [call for call in raw if valid(call)]
        invalid += len(raw) - len(calls)
        if any(secret in json.dumps(raw, ensure_ascii=False) for secret in record["forbidden"]):
            leaks += 1
        pairs, missed, extra = _assign(record["expected"], calls)
        # Honest-but-ambiguous readings are neither required nor penalised.
        _, _, extra = _assign(record.get("optional", []), extra)
        extra_invalid = len(raw) - len(calls)
        tp, fp, fn = len(pairs), len(extra) + extra_invalid, len(missed)
        overall.add(tp, fp, fn)
        by_category[record["category"]].add(tp, fp, fn)
        if record["suite"] == "translate":
            by_split["heldout formats" if record["heldout"] else "seen formats"].add(tp, fp, fn)
        for edge in record["edges"]:
            by_edge[edge].add(tp, fp, fn)
        for want, _ in pairs:
            by_function[want["name"]].add(1, 0, 0)
        for want in missed:
            by_function[want["name"]].add(0, 0, 1)
        for call in extra:
            by_function[call["name"]].add(0, 1, 0)
        exact[fp == 0 and fn == 0] += 1
        if not record["expected"] and not record.get("optional"):
            negatives += 1
            negatives_clean += not raw
        for call in calls:
            invented, checked = _grounding(record, call)
            invented_total += len(invented)
            checked_total += checked
        # Error spotted at all, whatever kind it was given.
        wanted_errors = sum(call["name"] == "error_detected" for call in record["expected"])
        errors_expected += wanted_errors
        errors_seen += min(wanted_errors, sum(call["name"] == "error_detected" for call in calls))
        for want, got in pairs:
            if want["name"] == "propose_recipe":
                defaults_total += 1
                wanted = [param["default"] for param in want["args"]["params"]]
                given = [param.get("default") for param in got["args"].get("params", [])]
                defaults_right += wanted == given
        if fp or fn:
            failures.append({"id": record["id"], "missed": missed, "extra": extra, "invalid": extra_invalid})
    report = {
        "examples": len(dataset),
        "overall": overall.row(),
        "exact_match": round(exact[True] / max(1, len(dataset)), 4),
        "invented_value_rate": round(invented_total / checked_total, 4) if checked_total else 0.0,
        "secret_leak_examples": leaks,
        "negative_screens_clean": round(negatives_clean / negatives, 4) if negatives else None,
        "invalid_calls": invalid,
        "missing_predictions": missing,
        "recipe_defaults_correct": round(defaults_right / defaults_total, 4) if defaults_total else None,
        "errors_spotted_any_kind": round(errors_seen / errors_expected, 4) if errors_expected else None,
        "latency_ms": {"p50": statistics.median(latencies), "p95": sorted(latencies)[int(0.95 * (len(latencies) - 1))]} if latencies else None,
        "by_function": {name: tally.row() for name, tally in sorted(by_function.items())},
        "by_category": {name: tally.row() for name, tally in sorted(by_category.items())},
        "by_format_split": {name: tally.row() for name, tally in sorted(by_split.items())},
        "by_edge": {name: tally.row() for name, tally in sorted(by_edge.items())},
    }
    return report, failures


def _table(title, rows):
    lines = [f"\n### {title}\n", "| | precision | recall | F1 | tp | fp | fn |", "|---|---|---|---|---|---|---|"]
    for name, row in rows.items():
        lines.append(f"| {name} | {row['precision']:.3f} | {row['recall']:.3f} | {row['f1']:.3f} | {row['tp']} | {row['fp']} | {row['fn']} |")
    return "\n".join(lines)


def markdown(report, edges):
    o = report["overall"]
    head = [
        f"**Examples:** {report['examples']} · **F1** {o['f1']:.3f} (precision {o['precision']:.3f}, recall {o['recall']:.3f}) · **exact match** {report['exact_match']:.1%}",
        f"**Invented values:** {report['invented_value_rate']:.1%} · **secret leaks:** {report['secret_leak_examples']} examples · "
        f"**clean negatives:** {report['negative_screens_clean']:.1%} · **invalid calls:** {report['invalid_calls']} · **missing:** {report['missing_predictions']}",
    ]
    if report["errors_spotted_any_kind"] is not None:
        head.append(f"**Errors spotted (any kind):** {report['errors_spotted_any_kind']:.1%} · kind must match for the error_detected row below")
    if report["recipe_defaults_correct"] is not None:
        head.append(f"**Recipe defaults correct:** {report['recipe_defaults_correct']:.1%}")
    if report["latency_ms"]:
        head.append(f"**Latency:** p50 {report['latency_ms']['p50']:.0f} ms · p95 {report['latency_ms']['p95']:.0f} ms")
    parts = ["\n".join(head), _table("By function", report["by_function"]), _table("By category", report["by_category"]), _table("Seen vs held-out formats", report["by_format_split"])]
    if edges:
        worst = dict(sorted(report["by_edge"].items(), key=lambda item: item[1]["f1"]))
        parts.append(_table("By edge case (worst first)", worst))
    return "\n".join(parts)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("dataset", type=Path)
    parser.add_argument("predictions", type=Path)
    parser.add_argument("--json", type=Path, help="write the full report here")
    parser.add_argument("--failures", type=Path, help="write per-example misses and false calls here")
    parser.add_argument("--edges", action="store_true", help="include the per-edge-case table")
    args = parser.parse_args()
    dataset = [json.loads(line) for line in args.dataset.read_text(encoding="utf-8").splitlines() if line.strip()]
    predictions = {}
    for line in args.predictions.read_text(encoding="utf-8").splitlines():
        if line.strip():
            entry = json.loads(line)
            predictions[entry["id"]] = entry
    report, failures = score(dataset, predictions)
    print(markdown(report, args.edges))
    if args.json:
        args.json.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    if args.failures:
        args.failures.write_text("".join(json.dumps(item) + "\n" for item in failures), encoding="utf-8")


if __name__ == "__main__":
    main()
