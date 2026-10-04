# Sentinal

Sentinal is a local-first control plane for terminal work across personal
machines. A Rust daemon owns or observes Sessions, records Commands and
Metrics, derives structured Events from terminal output, and exposes them to a
phone-friendly web app through a control Machine on your Tailnet.

Sentinal is not an AI terminal. Its protocol, session management, watcher,
routing, Recipes, and notifications work without a model. Optional local
intelligence remains suggestion-only future work.

## Current status

The Milestone 4.B feature set is implemented and has isolated automated
acceptance. This includes:

- daemon-owned persistent PTYs, detach/reattach, retained output, and Session
  continuation;
- Watch, shell-hook, auto-wrap, and read-only process/log adoption;
- Command history, search, process trees, CPU/RAM/NVIDIA Metrics, and
  structured training summaries;
- deterministic Events and Health for training, pytest, Cargo, and
  llama.cpp/vLLM output;
- secure Machine enrollment and control-origin routing across multiple
  independently credentialed workers;
- Machine-scoped terminal control, Recipes, ntfy notifications, and a mobile
  PWA interface;
- `sentinal top` and `sentinal status <SESSION_ID>` for terminal monitoring.

The repository is a **V1 release candidate**, not a production V1 release.
The remaining release gates are tracked in
[`docs/AUDIT_TRACKER.md`](docs/AUDIT_TRACKER.md): browser/WebSocket credential
transport, credential-refresh recovery, storage quotas and purge, login
throttling hardening, production PWA delivery, supported-host integration
evidence, and the recorded two-real-Machine acceptance run.

Sessions survive browser and CLI disconnects. They do **not** yet survive a
`sentinald` restart; live daemon-owned Sessions are marked interrupted while
their retained output remains available. Restart-resilient PTY ownership is
the separate Reliability R1 milestone.

## Architecture

```text
phone / desktop browser
        │ owner session, one control origin
        ▼
control sentinald ───── independently authenticated routes ────┐
        │                                                       │
        ▼                                                       ▼
 worker sentinald                                          worker sentinald
 PTYs · Commands · Metrics · deterministic Events          PTYs · Events
```

The layers remain one-way:

```text
protocol → daemon/store → watcher → web/CLI → optional intelligence
```

The browser never receives worker credentials and never connects directly to
a worker. The control daemon proves the selected worker's immutable Machine ID
before proxying terminal or data traffic.

## Development quick start

Requirements are Rust stable, Node.js, pnpm, and optionally
[`just`](https://github.com/casey/just).

```bash
pnpm -C web install
just check
just test-e2e
```

`just check` runs Rust formatting, all-feature Clippy with warnings denied,
workspace tests, generated TypeScript verification, web type/tests, evaluator
tests, and offline shell-script checks. `just test-e2e` runs the isolated
Playwright acceptance suite. Use `just acceptance` to run both.

Useful development commands:

```bash
just network --dry-run       # inspect the Tailscale launch configuration
just network                 # daemon + real-data UI on one Tailnet Machine
just agent                   # worker daemon only
just control WORKER_IP=100.x.y.z
just demo                    # explicitly fixture-backed UI
```

These are source-tree development paths. A production-built PWA service is a
remaining V1 release gate.

## CLI

```bash
sentinal watch -- python train.py --config config.toml
sentinal ls
sentinal top
sentinal status 1
sentinal attach 1
sentinal adopt --pick
sentinal hook install zsh
sentinal auto on zsh
sentinal connect https://control.example.ts.net --name "RTX PC"
```

The installed programs are `sentinal` and `sentinald`. The legacy `ta` and
`tad` binaries remain compatibility aliases.

## Installation and security

Sentinal accepts loopback or Tailscale addresses and refuses wildcard and
ordinary-LAN binds. Installation and Machine enrollment are separate actions:

```text
install → UNPAIRED → sentinal connect → PAIRED
```

Start with [`docs/SETUP.md`](docs/SETUP.md) for worker installation, Tailnet
policy, control setup, enrollment, backup, and acceptance steps. Read
[`docs/SECURITY.md`](docs/SECURITY.md) before exposing terminal control to a
phone or another Machine. Do not treat the current Vite development launcher
as a public-internet deployment path.

## Repository guide

| Path | Purpose |
|---|---|
| `crates/protocol` | Shared Rust/JSON contract and fixtures |
| `crates/daemon` | PTYs, routing, auth, Recipes, notifications, Metrics |
| `crates/store` | SQLite persistence, history, registry, retention |
| `crates/watcher` | Deterministic output-to-Event adapters |
| `crates/cli` | `sentinal` command-line client |
| `web` | React, xterm.js, mobile PWA, Vitest, Playwright |
| `recipes` | Built-in Recipe TOML files |
| `eval` | Deterministic intelligence dataset generation and scoring |
| `design_handoff_sentinal` | Source design references and assets |
| `marketing/ad` | Reproducible source for product films |
| `docs` | Architecture, security, setup, roadmap, and progress evidence |

Read [`AGENTS.md`](AGENTS.md) before changing code. The current build order is
in [`docs/ROADMAP.md`](docs/ROADMAP.md), testing rules are in
[`docs/TESTING.md`](docs/TESTING.md), and the independent remediation status is
in [`docs/AUDIT_TRACKER.md`](docs/AUDIT_TRACKER.md).
