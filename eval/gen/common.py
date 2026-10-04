"""Lines that carry their own ground truth, and the expected answer they imply."""

import random
import string
import sys
from dataclasses import dataclass, field
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
from canon import metric_name, unit_group  # noqa: E402

SCREEN_LINES = 40


@dataclass
class Line:
    """One output line and the facts a perfect reader would take from it."""

    text: str
    facts: list = field(default_factory=list)


def progress(current, total, unit):
    return ("progress", current, total, unit)


def metric(name, printed):
    """`printed` is the exact string on screen; the expected value is parsed from it."""
    return ("metric", name, printed)


def checkpoint(path):
    return ("checkpoint", path)


def error(kind):
    return ("error", kind)


def warning():
    return ("warning",)


def prompt(kind, text):
    return ("prompt", kind, text)


def server(port, url):
    return ("server", port, url)


def tests(passed, failed, skipped):
    return ("tests", passed, failed, skipped)


def optional(call):
    """A call that is honest but not required, e.g. a test-failure error beside its summary."""
    return ("optional_call", call)


def secret(value):
    return ("secret", value)


def expected_calls(lines):
    """The calls a perfect translator makes for the visible `lines`.

    Latest progress per unit group and latest value per metric; every
    checkpoint, error kind, and warning; the newest server and test summary;
    a prompt only when it is the last non-empty line.
    """
    progress_by_unit = {}
    metrics = {}
    checkpoints = []
    errors = []
    warnings = 0
    server_call = None
    tests_call = None
    last_text = next((line for line in reversed(lines) if line.text.strip()), None)
    for line in lines:
        for fact in line.facts:
            kind = fact[0]
            if kind == "progress":
                _, current, total, unit = fact
                progress_by_unit[unit_group(unit)] = {"current": current, "total": total, "unit": unit}
            elif kind == "metric":
                _, name, printed = fact
                metrics[metric_name(name)] = {"name": metric_name(name), "value": float(printed.replace(",", "").rstrip("%"))}
            elif kind == "checkpoint" and fact[1] not in checkpoints:
                checkpoints.append(fact[1])
            elif kind == "error" and fact[1] not in errors:
                errors.append(fact[1])
            elif kind == "warning":
                warnings += 1
            elif kind == "server":
                server_call = {"port": fact[1], "url": fact[2]}
            elif kind == "tests":
                tests_call = {"passed": fact[1], "failed": fact[2], "skipped": fact[3]}
    calls = [{"name": "progress_updated", "args": args} for args in progress_by_unit.values()]
    calls += [{"name": "metric_reported", "args": args} for args in metrics.values()]
    calls += [{"name": "checkpoint_created", "args": {"path": path}} for path in checkpoints]
    calls += [{"name": "error_detected", "args": {"kind": kind}} for kind in errors]
    calls += [{"name": "warning_detected", "args": {}} for _ in range(warnings)]
    if server_call:
        calls.append({"name": "server_started", "args": server_call})
    if tests_call:
        calls.append({"name": "tests_summary", "args": tests_call})
    if last_text is not None:
        for fact in last_text.facts:
            if fact[0] == "prompt":
                calls.append({"name": "waiting_for_input", "args": {"kind": fact[1], "prompt": fact[2]}})
    return calls


def optional_calls(lines):
    """Calls that are neither required nor penalised (honest but ambiguous readings)."""
    latest = {}
    others = []
    for line in lines:
        for fact in line.facts:
            if fact[0] == "optional_progress":
                latest[unit_group(fact[3])] = {"name": "progress_updated", "args": {"current": fact[1], "total": fact[2], "unit": fact[3]}}
            elif fact[0] == "optional_call" and fact[1] not in others:
                others.append(fact[1])
    return list(latest.values()) + others


def forbidden(lines):
    return sorted({fact[1] for line in lines for fact in line.facts if fact[0] == "secret"})


def example(example_id, category, family, edges, command, lines, source="screen", heldout=False, note=None):
    """A translate-suite example over the visible tail of `lines`."""
    visible = lines[-SCREEN_LINES:]
    record = {
        "id": example_id,
        "suite": "translate",
        "category": category,
        "family": family,
        "heldout": heldout,
        "edges": sorted(set(edges + (["scrolled"] if len(lines) > SCREEN_LINES else []))),
        "input": {"command": command, "source": source, "screen": "\n".join(line.text for line in visible)},
        "expected": expected_calls(visible),
        "forbidden": forbidden(visible),
        "optional": optional_calls(visible),
    }
    if note:
        record["note"] = note
    return record


# ---- fake secrets (realistic shapes, never real) -----------------------------

def _token(rng, alphabet, length):
    return "".join(rng.choice(alphabet) for _ in range(length))


def fake_secret(rng):
    alnum = string.ascii_letters + string.digits
    choice = rng.randrange(6)
    if choice == 0:
        return "hf_" + _token(rng, alnum, 34)
    if choice == 1:
        return "sk-" + _token(rng, alnum, 48)
    if choice == 2:
        return "AKIA" + _token(rng, string.ascii_uppercase + string.digits, 16)
    if choice == 3:
        return "ghp_" + _token(rng, alnum, 36)
    if choice == 4:
        return "xoxb-" + _token(rng, string.digits, 12) + "-" + _token(rng, alnum, 24)
    return "Pa55-" + _token(rng, alnum, 14)


def secret_line(rng):
    """A line leaking a secret, as real terminals sometimes do."""
    value = fake_secret(rng)
    templates = [
        "export HF_TOKEN={v}",
        "OPENAI_API_KEY={v} python app.py",
        "Authorization: Bearer {v}",
        "DATABASE_URL=postgres://admin:{v}@db.internal:5432/app",
        "wandb: Appending key for api.wandb.ai to your netrc file: {v}",
        "git remote add origin https://oauth2:{v}@gitlab.com/team/repo.git",
    ]
    return Line(rng.choice(templates).format(v=value), [secret(value)])


# ---- number printing -----------------------------------------------------------

def print_float(rng, value, style=None):
    """Print `value` the many ways training code does."""
    style = style or rng.choice(["f2", "f3", "f4", "f4", "g"])
    if style == "f2":
        return f"{value:.2f}"
    if style == "f3":
        return f"{value:.3f}"
    if style == "f4":
        return f"{value:.4f}"
    return f"{value:.4g}"


def print_rate(rng, value):
    return rng.choice([f"{value:.1e}", f"{value:.0e}", f"{value:.2E}", f"{value:.6f}".rstrip("0")])


FILLER = [
    "Loading tokenizer...",
    "Resolving dependencies...",
    "Reading configuration from ./settings.toml",
    "Initialising workers",
    "Using cache at ~/.cache/sentinal",
    "Connected.",
    "Done.",
]


def filler(rng, most=3):
    """Neutral context lines that carry no facts."""
    return [Line(text) for text in rng.sample(FILLER, rng.randrange(0, most + 1))]


def scramble_digits(rng, text):
    """Same shape, different numbers: for lines whose numbers are not facts."""
    return "".join(rng.choice("0123456789") if character.isdigit() else character for character in text)


def make_rng(seed, *parts):
    return random.Random(f"{seed}:" + ":".join(str(part) for part in parts))
