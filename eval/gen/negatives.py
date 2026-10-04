"""Screens full of numbers and keywords where nothing should be reported."""

from .common import Line, example, scramble_digits, secret_line

SCREENS = [
    ("ls", "ls -la", [
        "total 48",
        "drwxr-xr-x  6 sohith staff   192 Oct  4 11:02 .",
        "-rw-r--r--  1 sohith staff  8412 Oct  3 09:14 train.py",
        "-rw-r--r--  1 sohith staff 52113 Oct  4 10:58 epoch_10.pt",
        "-rw-r--r--  1 sohith staff   931 Oct  1 17:40 config.yaml",
    ]),
    ("git_log", "git log --oneline -5", [
        "9cb5a95 feat: Recipes, notifications, Machine view (4.B)",
        "bdfeff9 feat: add multi-machine aggregation and routing",
        "e5ca0aa feat(web): animated \"Copied to clipboard\" confirmation",
        "c89e78c fix: loss of focus after 3/4 reconnects",
        "58bd371 feat(launcher): `just network` with port 7420",
    ]),
    ("nvidia_smi", "nvidia-smi", [
        "+-----------------------------------------------------------------------------------------+",
        "| NVIDIA-SMI 570.86.15              Driver Version: 570.86.15      CUDA Version: 12.8     |",
        "| GPU  Name                 Persistence-M | Bus-Id          Disp.A | Volatile Uncorr. ECC |",
        "|   0  NVIDIA GeForce RTX 5060 Ti     Off |   00000000:01:00.0  On |                  N/A |",
        "|  0%   31C    P8             10W /  180W |     241MiB /  16311MiB |      0%      Default |",
        "|    0   N/A  N/A      2594      G   /usr/lib/xorg/Xorg                            128MiB |",
    ]),
    ("config_cat", "cat config.yaml", [
        "model: Qwen/Qwen3-0.6B",
        "epochs: 10",
        "lr: 2.0e-4",
        "loss: cross_entropy",
        "save_every: 500",
        "output_dir: checkpoints/run-7",
    ]),
    ("ping", "ping -c 3 1.1.1.1", [
        "PING 1.1.1.1 (1.1.1.1): 56 data bytes",
        "64 bytes from 1.1.1.1: icmp_seq=0 ttl=57 time=12.304 ms",
        "64 bytes from 1.1.1.1: icmp_seq=1 ttl=57 time=11.874 ms",
        "3 packets transmitted, 3 packets received, 0.0% packet loss",
    ]),
    ("docker_ps", "docker ps", [
        "CONTAINER ID   IMAGE          COMMAND                  STATUS        PORTS                    NAMES",
        "3f2a1b9c8d7e   postgres:16    \"docker-entrypoint.s…\"   Up 2 hours    0.0.0.0:5432->5432/tcp   db",
        "8e7d6c5b4a39   vllm/vllm:0.6  \"python3 -m vllm.ent…\"   Up 3 minutes  0.0.0.0:8000->8000/tcp   llm",
    ]),
    ("df", "df -h", [
        "Filesystem      Size  Used Avail Use% Mounted on",
        "/dev/nvme0n1p7  425G  401G  2.3G 100% /",
        "tmpfs            16G  2.1M   16G   1% /run",
    ]),
    ("readme", "less README.md", [
        "## Training",
        "Run `python train.py --epochs 10`. Expect the loss to reach about 0.35 by epoch 8/10.",
        "Checkpoints are saved to `checkpoints/` every 500 steps.",
        "If you see `CUDA out of memory`, lower the batch size.",
    ]),
    ("history", "history | tail -4", [
        "  501  python train.py --lr 2e-4 --epochs 10",
        "  502  nvidia-smi",
        "  503  tail -f logs/train.log",
        "  504  history | tail -4",
    ]),
    ("top", "top -l 1 -n 3", [
        "Processes: 612 total, 3 running, 609 sleeping, 3021 threads",
        "CPU usage: 12.50% user, 6.25% sys, 81.25% idle",
        "PhysMem: 15G used (2104M wired), 1024M unused.",
        "PID    COMMAND      %CPU TIME     #TH",
        "4242   python3.11   97.5 01:23:45 12",
    ]),
    ("questions", "python check_env.py", [
        "Is the GPU available? True",
        "Should we resume? Checking for checkpoints... none found",
        "CUDA devices: 2",
        "Environment OK",
    ]),
    ("grep_errors", "grep -c ERROR logs/*.log", [
        "logs/train-2026-10-01.log:0",
        "logs/train-2026-10-02.log:0",
        "logs/serve.log:0",
    ]),
]


def generate(rng, index):
    family, command, texts = rng.choice(SCREENS)
    keep = sorted(rng.sample(range(len(texts)), rng.randrange(max(1, len(texts) - 2), len(texts) + 1)))
    lines = [Line(scramble_digits(rng, texts[position]) if rng.random() < 0.7 else texts[position]) for position in keep]
    edges = [family, "negative"]
    if rng.random() < 0.15:
        lines.insert(rng.randrange(len(lines) + 1), secret_line(rng))
        edges.append("secret")
    return example(f"negative-{family}-{index:04d}", "negatives", family, edges, command, lines)
