# Sentinel development plan

## Product

Sentinel is a local-first terminal orchestration and intelligence system. It
is not another terminal emulator: it is the layer that makes terminal work on
several machines persistent, observable, searchable, and remotely reachable.

User-facing names:

- Product: **Sentinel**
- CLI: `sentinel` (currently `ta` until the Milestone 1b migration)
- Daemon: `sentineld` (currently `tad` until the Milestone 1b migration)

Sentinel answers five everyday questions:

1. What am I running?
2. How is it going?
3. Can I reach and control it remotely?
4. How did I run this last time?
5. Can I bring this ordinary terminal or existing process into Sentinel now?

The fifth question is served by Sentinel Watch and reduced-capture modes:

```console
$ sentinel watch -- python train.py --config moe.yaml
```

The central engineering rule is **structured state first, intelligence
second**. No layer below the optional intelligence service may require an AI
model.

## Product model

The stable vocabulary is Machine, Project, Session, Command, Event, Recipe,
Process, and Metric.

- A **Machine** is one computer running `sentineld`, including its resources,
  capabilities, online state, and Sessions.
- A **Project** groups repository metadata, Sessions, Commands, Events, and
  later agent context.
- A **Session** represents persistent or observed work and carries its PTY,
  shell/process identity, cwd, state, process tree, screen/log when available,
  origin, and capture level.
- A **Command** records exact text, cwd, Machine, Project, timestamps, exit
  status, duration, and deterministic classification.
- An **Event** is structured meaning derived from Commands, output, process
  state, Metrics, and later watcher adapters.
- A **Recipe** is a validated, shell-quoted reusable Command template.
- A **Process** is an observed OS process in a Session tree.
- A **Metric** is a timestamped Machine or Session resource measurement.

Session lifecycle states remain `CREATED`, `IDLE`, `RUNNING`, `WAITING`,
`COMPLETED`, `FAILED`, `INTERRUPTED`, `STALLED`, and `DISCONNECTED`.

Milestone 1b adds two orthogonal Session dimensions:

- origin: `UI`, `WATCH`, `SHELL_HOOK`, `AUTO_WRAP`, or `ADOPTED`
- capture: `FULL`, `COMMANDS`, `PROCESS_LOG`, or `PROCESS`

These additions are planned but are not yet present in the protocol. Their
wire representation and fixtures are the first Milestone 1b task.

## Architecture

Sentinel has four layers:

1. **Interfaces** — web/PWA, CLI, and later terminal UI.
2. **Control plane** — Machines, Projects, Sessions, Commands, Recipes,
   search, permissions, and notifications.
3. **Deterministic intelligence** — watcher adapters, Event engine, progress,
   health, and summaries derived without a model.
4. **Machine agent** — PTYs, shells, processes, CPU/RAM/GPU, history, Git, and
   filesystem metadata.

The optional model-backed intelligence service sits beside these layers as a
normal API client. Initially it may observe, summarize, and suggest only.

## Delivery plan

### Phase 0 — contract and specification (complete)

Define the data model, lifecycle, wire envelope, fixtures, generated
TypeScript, architecture, testing rules, and dependency policy.

### Phase 1 — persistent Sessions (complete)

Create, attach, detach, reconnect, resize, input, kill, list, retain screen
state and scrollback, and detect exit. Sessions survive UI disconnection.

### Phase 1.1 — single-owner authentication (complete)

Bootstrap with the Machine token, store one Argon2id password hash, issue
expiring browser sessions, rate-limit password attempts, and support logout
and password rotation.

### Phase 2 — Command history and Metrics (complete)

Auto-source OSC shell hooks in daemon-owned PTYs, track exact Commands and
cwd, persist SQLite history and FTS5, recover orphaned Sessions, sample
process trees and resources, and stream live Metrics to the web UI.

### Phase 1b — any terminal into Sentinel (next priority)

Bring ordinary terminals and already-running processes into Sentinel through
four explicit modes:

1. **Watch** — on-demand FULL capture with a daemon-owned persistent PTY.
2. **Shell hook** — COMMANDS capture from an ordinary terminal.
3. **Auto-wrap** — opt-in FULL capture for new interactive terminals.
4. **Adopt** — read-only PROCESS or PROCESS_LOG observation of an existing
   same-user process.

This phase is divided into contract/local ingress, Watch, external hook,
auto-wrap/adopt, and watcher-backed log tail stages. Safety and test gates are
normative in `CAPTURE_MODES.md`, `SECURITY.md`, and `TESTING.md`.

### Phase 3 — Event engine and deterministic watchers

Turn output, process state, Metrics, Commands, and exit status into Events.
Ship a generic watcher, training/pytest/vLLM adapters, declarative adapters,
and adopted-log ingestion.

### Phase 4 — Recipes, notifications, and multi-Machine dashboard

Launch built-in or saved Recipes, aggregate per-Machine APIs in the PWA, and
deliver event-driven ntfy notifications.

### V2 — deterministic completion and local intelligence

Add Project grouping, ranked autocomplete from history and Recipes,
notification policy, model evals, Level 0–2 local-agent summaries, an
operational journal, and “What happened while I was gone?”

### V3 — developer activity model

Unify IDE, Git, terminal, build, test, training, and server Events into a
local developer activity model. Model-backed reasoning remains optional.

## V1 boundary

V1 contains the daemon, persistent Sessions, all four capture modes, Command
tracking, adoption, process/resource monitoring, deterministic Events and a
basic watcher, web/mobile dashboard, remote attach, and Recipes. It does not
contain autonomous execution or sophisticated AI.

When V1 is complete, the owner can open a phone, see every Machine and
Session—including work started outside Sentinel—inspect Commands, progress,
and resources, attach where capture is FULL, and launch a saved Recipe.

## Implementation order

```text
protocol/data model (complete)
  → PTY persistence and attach (complete)
  → Command tracking, SQLite, Metrics (complete)
  → Milestone 1b contract + private local ingress
  → Sentinel Watch
  → external shell hook
  → auto-wrap + read-only adopt
  → Event engine + generic watcher
  → adopted log tail
  → multi-Machine/PWA + Recipes
  → autocomplete + specialized watchers + notifications
  → Projects
  → optional local agent + journal + suggestions
```

## Repository direction

The existing Cargo workspace remains authoritative. User-facing renames do
not require flattening its crates:

```text
crates/protocol    shared contract
crates/store       SQLite persistence
crates/daemon      sentineld implementation
crates/cli         sentinel implementation
crates/watcher     deterministic output → Events (Phase 3)
shell/             zsh/bash/fish integrations
web/               React PWA
recipes/           built-in Recipe TOML
intelligence/      optional Python service and evals
```

## Required documents

Each milestone updates every affected document before it is complete.

| Document | File | Status |
|---|---|---|
| Development plan | `docs/PLAN.md` | current |
| Agent/coding rules | `AGENTS.md` | existing |
| Architecture | `docs/ARCHITECTURE.md` | current |
| Roadmap/checklists | `docs/ROADMAP.md` | current |
| Testing guide | `docs/TESTING.md` | current |
| Approved dependencies | `docs/DEPENDENCIES.md` | current |
| Protocol/data model | `crates/protocol` + fixtures | current through M2; capture additions pending 1b |
| Capture modes | `docs/CAPTURE_MODES.md` | specified; implementation pending |
| Security/privacy | `docs/SECURITY.md` | current + planned guarantees labelled |
| CLI reference | `docs/CLI.md` | current commands + 1b target specified |
| Shell hooks | `docs/SHELL_HOOKS.md` | current OSC path + external path specified |
| Setup/Tailscale/PWA | `docs/SETUP.md` | due Phase 4 multi-Machine work |
| Design sources | `design_handoff_sentinal/design/` | in progress |
| Eval results | `docs/EVALS.md` | due V2 intelligence work |
| Milestone reports | `docs/progress/<milestone>.md` | per milestone |
