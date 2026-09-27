# Roadmap

Work top to bottom. Tick boxes as they land. Each milestone ends with a
demo you can actually use.

## Milestone 0 — Protocol (the contract)
- [x] Workspace, rules, CI, testing guide
- [x] `Machine`, `Session` + `SessionState` state machine
- [x] `Command` (with duration and outcome)
- [x] `Event` + `EventKind`
- [x] `Recipe` with parameter rendering and validation
- [x] `Envelope` with protocol version
- [x] Contract fixtures for every wire type
- [x] `Process` and `Metric` types (needed by Milestone 2)
- [x] Generate TypeScript types (`ts-rs`) for the web app

**Done when:** `just check` passes and every wire type has a fixture.

## Milestone 1 — Persistent sessions (v0.1)
- [x] `daemon` crate: spawn shell in a PTY, keep it alive
- [x] Create / list / attach / detach / resize / input / kill over WebSocket
- [x] Screen state via VT parser, sent on reattach
- [x] Scrollback ring buffer + compressed on-disk log
- [x] Minimal `cli` to attach from another terminal
- [x] Minimal web page with xterm.js
- [x] Safe daemon config, localhost/Tailscale binding, token auth, audit log
- [x] systemd and launchd user service files
- [x] Fast fake-backed tests plus ignored real-PTY and Playwright smoke tests

**Demo:** start `python long_job.py`, close everything, reattach from your phone.

## Milestone 2 — Command tracking + metrics (v0.2)
- [ ] Shell hooks (zsh, bash, fish) emitting OSC 133
- [ ] Command start/end/exit code parsed from the PTY stream
- [ ] `store` crate: SQLite history with FTS5 search
- [ ] Per-session process tree, CPU, RAM, GPU (NVIDIA)

**Demo:** history view with ✓/✗, durations, and live resource usage.

## Milestone 3 — Events + watcher (v0.3)
- [ ] Event engine (raw observations → `Event`)
- [ ] Generic watcher (idle, waiting for input, possibly stalled)
- [ ] Adapters: training, pytest, vLLM
- [ ] Declarative TOML regex adapters

## Milestone 4 — Recipes + mobile dashboard (v0.4)
- [ ] Recipe files in `recipes/`, launch from UI
- [ ] "Save as recipe" after a successful command
- [ ] Multi-machine view (each daemon serves its own API; UI aggregates)
- [ ] ntfy notifications for completed / failed / waiting / stalled

## V2 — Intelligence
- [ ] Tool API for the agent (same permissions as a user, capped at Level 2)
- [ ] Eval harness with hand-labelled sets
- [ ] Fine-tune small model (FunctionGemma 270M / Qwen3-0.6B) for
      NL → tool call routing, then command classification, then triage
- [ ] Heartbeat, journal, summaries
