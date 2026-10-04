# Sentinal development plan

## Product

Sentinal is a local-first terminal orchestration and intelligence system. It
is not another terminal emulator: it is the layer that makes terminal work on
several machines persistent, observable, searchable, and remotely reachable.

User-facing names:

- Product: **Sentinal**
- CLI: `sentinal` (`ta` remains a compatibility alias)
- Daemon: `sentinald` (`tad` remains a compatibility alias)

Sentinal answers five everyday questions:

1. What am I running?
2. How is it going?
3. Can I reach and control it remotely?
4. How did I run this last time?
5. Can I bring this ordinary terminal or existing process into Sentinal now?

The fifth question is served by Sentinal Watch and reduced-capture modes:

```console
$ sentinal watch -- python train.py --config moe.yaml
```

The central engineering rule is **structured state first, intelligence
second**. No layer below the optional intelligence service may require an AI
model.

## Product model

The stable vocabulary is Machine, Project, Session, Command, Event, Recipe,
Process, and Metric.

- A **Machine** is one computer running `sentinald`, including its resources,
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

These additions are implemented in the protocol with contract fixtures and
generated TypeScript. Daemon and web capability checks remain authoritative.

## Architecture

Sentinal has four layers:

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

### Phase 1b — any terminal into Sentinal (complete)

Bring ordinary terminals and already-running processes into Sentinal through
four explicit modes:

1. **Watch** — on-demand FULL capture with a daemon-owned persistent PTY.
2. **Shell hook** — COMMANDS capture from an ordinary terminal.
3. **Auto-wrap** — opt-in FULL capture for new interactive terminals.
4. **Adopt** — read-only PROCESS or PROCESS_LOG observation of an existing
   same-user process.

This phase is divided into contract/local ingress, Watch, external hook,
auto-wrap/adopt, watcher-backed log tail, and finished-Session replay stages.
Safety and test gates are normative in `CAPTURE_MODES.md`, `SECURITY.md`, and
`TESTING.md`.

### Phase 1c — deployable worker and two-host acceptance (complete)

Package the existing daemon, CLI, hooks, and user service as one worker bundle.
Provide distinct same-Machine, daemon-only worker, and remote control-UI launch
roles, plus a tested installer and exact Tailscale setup guide. This proves one
selected worker from a second Machine; it does not claim aggregation.

### Phase 1c.1 — Tailnet bootstrap and private package delivery (complete)

Designate one owner-controlled `sentinald` as the local control Machine and
let it serve verified Linux/macOS worker releases inside the Tailnet. The
bootstrap URL installs binaries, hooks, config, and the user service, but
does not enroll the Machine or carry any pairing secret:

```console
curl -fsSL https://sentinal-control.<tailnet>.ts.net/install.sh | sh
```

Installation and enrollment are deliberately separate. The lifecycle is:

```text
install → UNPAIRED → connect → PAIRED
```

Reinstall and update preserve Machine identity, credentials, SQLite, logs,
adapters, and recipes. Package delivery is Tailnet-local infrastructure, not
a public package repository or cloud requirement.

### Phase 3 — Event engine and deterministic watchers (complete)

Turn output, process state, Metrics, Commands, and exit status into Events.
Ship a generic watcher, training/pytest/vLLM adapters, declarative adapters,
and adopted-log ingestion.

### Phase 4 — connect, enrollment, multi-Machine dashboard, Recipes, notifications

The 4.0, 4.A, and 4.B implementations and isolated browser acceptance are
complete. Production PWA delivery, security remediation, supported-host
integration evidence, and the real two-Machine V1 run remain release gates.

After installation, `sentinal connect` explicitly pairs the worker with the
given control URL. The UI creates a short-lived, single-use code; the CLI
reads it from a hidden prompt or stdin, never argv or a URL. Connect presents
the worker's stable Machine ID and requested display name, then exchanges the
code for a unique revocable Machine credential. The Machine token stays local.
The control daemon now aggregates enrolled Machines, proves worker identity,
routes actions to the owning Machine, launches built-in or saved Recipes, and
delivers event-driven ntfy notifications without exposing worker credentials
to the browser.

### Reliability R1 — daemon-restart-resilient FULL Sessions

UI/CLI disconnect persistence is implemented. Surviving a `sentinald` process
restart is a separate architecture change because the daemon currently owns
the PTY. The planned review compares keeper/supervisor designs and must be
approved before PTY ownership moves. Until R1 lands, restart recovery safely
marks daemon-owned Sessions interrupted.

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

That feature boundary is implemented. The repository remains a V1 release
candidate until the checklist in `docs/AUDIT_TRACKER.md` is complete. In
particular, “persistent” currently means across client disconnects: a
`sentinald` restart interrupts daemon-owned live PTYs until Reliability R1.

When V1 is complete, the owner can open a phone, see every Machine and
Session—including work started outside Sentinal—inspect Commands, progress,
and resources, attach where capture is FULL, and launch a saved Recipe.

## Implementation order

```text
protocol/data model (complete)
  → PTY persistence and attach (complete)
  → Command tracking, SQLite, Metrics (complete)
  → all capture modes + retained replay (complete)
  → worker packaging + private delivery (complete)
  → Event engine + deterministic watchers (complete)
  → secure Machine enrollment + aggregation (implemented; real acceptance pending)
  → Recipes + notifications + mobile UI (implemented; release gates pending)
  → V1 audit remediation + production PWA delivery
  → Projects
  → autocomplete and additional specialized watchers
  → optional local agent + journal + suggestions
```

## Repository direction

The existing Cargo workspace remains authoritative. User-facing renames do
not require flattening its crates:

```text
crates/protocol    shared contract
crates/store       SQLite persistence
crates/daemon      sentinald implementation
crates/cli         sentinal implementation
crates/watcher     deterministic output → Events (Phase 3)
shell/             zsh/bash/fish integrations
web/               React PWA
recipes/           built-in Recipe TOML
eval/              deterministic model-evaluation foundation
intelligence/      optional Python service (V2, not yet created)
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
| Protocol/data model | `crates/protocol` + fixtures | current through 4.B |
| Capture modes | `docs/CAPTURE_MODES.md` | all four modes and adopted log tail implemented |
| Security/privacy | `docs/SECURITY.md` | current + planned guarantees labelled |
| CLI reference | `docs/CLI.md` | current commands + 1b target specified |
| Shell hooks | `docs/SHELL_HOOKS.md` | current OSC path + external path specified |
| Setup/Tailscale/PWA | `docs/SETUP.md` | current through 4.B; production PWA delivery pending |
| Design sources | `design_handoff_sentinal/design/` | retained source handoff; production UI is under `web/` |
| Eval foundation | `eval/README.md` | deterministic generator, scorer, and rules baseline |
| Milestone reports | `docs/progress/<milestone>.md` | per milestone |
