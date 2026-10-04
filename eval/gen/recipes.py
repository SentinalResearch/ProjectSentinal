"""Command histories and the Recipes a perfect assistant would propose.

Rule (deterministic, so answers are checkable): a command shape run successfully
at least MIN_RUNS times becomes a Recipe. Values that changed between those runs
become parameters defaulting to the latest value; values that never changed stay
literal. Failed runs do not count, and secrets never appear in a proposal.
"""

import re

from .common import fake_secret

MIN_RUNS = 3

FAMILIES = [
    ("train", "python train.py --lr {lr} --epochs {epochs} --batch-size {batch}", {
        "lr": ["1e-4", "2e-4", "3e-4", "5e-5"], "epochs": ["3", "10", "20"], "batch": ["16", "32"]}),
    ("llama", "llama-server -m {model} --port {port} -c {ctx}", {
        "model": ["models/qwen3-8b-q4.gguf", "models/llama3.1-8b-q8.gguf", "models/phi4-q4.gguf"], "port": ["8080", "8081", "9000"], "ctx": ["4096", "8192"]}),
    ("eval", "CUDA_VISIBLE_DEVICES={gpu} python eval.py --checkpoint {ckpt}", {
        "gpu": ["0", "1"], "ckpt": ["checkpoints/epoch_5.pt", "checkpoints/epoch_10.pt", "checkpoints/best.pt"]}),
    ("download", "huggingface-cli download {repo} --local-dir {dir}", {
        "repo": ["Qwen/Qwen3-8B", "google/gemma-3-270m", "meta-llama/Llama-3.1-8B"], "dir": ["models/hf", "/data/models"]}),
    ("pytest", "pytest tests/{module} -x -q", {
        "module": ["test_api.py", "test_db.py", "test_auth.py"]}),
    ("compose", "docker compose -f docker-compose.{env}.yml up -d {service}", {
        "env": ["dev", "staging"], "service": ["api", "worker", "web"]}),
    ("sync", "rsync -av --progress {src} pop-os:/data/{dst}/", {
        "src": ["runs/run-7/", "runs/run-8/", "datasets/clean/"], "dst": ["runs", "datasets"]}),
    ("vllm", "vllm serve {model} --max-model-len {len} --gpu-memory-utilization {util}", {
        "model": ["Qwen/Qwen3-8B", "Qwen/Qwen3-4B"], "len": ["8192", "16384", "32768"], "util": ["0.85", "0.9"]}),
]

NOISE = ["ls", "cd ~/Projects/TIM", "git status", "nvidia-smi", "cat config.yaml", "vim train.py", "htop", "git pull", "clear", "df -h"]


def _render(template, values):
    return template.format(**values)


def _expected(template, runs):
    """The proposal implied by successful runs of one family (oldest first)."""
    names = re.findall(r"\{(\w+)\}", template)
    varying = [name for name in names if len({run[name] for run in runs}) > 1]
    latest = runs[-1]
    literal = {name: latest[name] for name in names if name not in varying}
    shown = template.format(**{**{name: "{" + name + "}" for name in varying}, **literal})
    return {"template": shown, "params": [{"name": name, "default": latest[name]} for name in varying]}


def generate(rng, index):
    families = rng.sample(FAMILIES, rng.choice([0, 1, 1, 2, 2, 3]))
    history = []
    expected = []
    edges = []
    for key, template, choices in families:
        count = rng.choice([1, 2, 3, 3, 4, 5, 6])
        fixed = {name: rng.choice(values) for name, values in choices.items() if rng.random() < 0.35}
        runs = []
        for _ in range(count):
            values = {name: fixed.get(name, rng.choice(values)) for name, values in choices.items()}
            failed = rng.random() < 0.15
            history.append({"command": _render(template, values), "cwd": "~/Projects/TIM", "exit_code": 1 if failed else 0, "_family": key, "_values": values})
            if not failed:
                runs.append(values)
        if len(runs) >= MIN_RUNS:
            expected.append(_expected(template, runs))
            if count > len(runs):
                edges.append("failed_runs_ignored")
            if fixed:
                edges.append("constant_values_stay_literal")
        elif count >= 2:
            edges.append("below_threshold")
    for _ in range(rng.randrange(2, 9)):
        history.append({"command": rng.choice(NOISE), "cwd": "~/Projects/TIM", "exit_code": 0})
    forbidden = []
    if rng.random() < 0.2:
        token = fake_secret(rng)
        history.append({"command": f"export HF_TOKEN={token}", "cwd": "~", "exit_code": 0})
        forbidden.append(token)
        edges.append("secret")
    # Keep each family's runs in order while interleaving everything.
    order = sorted(range(len(history)), key=lambda position: (rng.random(), position))
    by_family = {}
    for position in order:
        by_family.setdefault(history[position].get("_family"), []).append(position)
    queues = {family: sorted(positions) for family, positions in by_family.items()}
    shuffled = []
    for position in order:
        family = history[position].get("_family")
        shuffled.append(history[queues[family].pop(0)])
    if not expected:
        edges.append("nothing_to_propose")
    if len(expected) > 1:
        edges.append("multiple_families")
    return {
        "id": f"recipes-{index:04d}",
        "suite": "recipes",
        "category": "recipes",
        "family": "history",
        "heldout": False,
        "edges": sorted(set(edges)),
        "input": {"history": [{key: value for key, value in entry.items() if not key.startswith("_")} for entry in shuffled]},
        "expected": [{"name": "propose_recipe", "args": proposal} for proposal in expected],
        "forbidden": forbidden,
        "optional": [],
    }
