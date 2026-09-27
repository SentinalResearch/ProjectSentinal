# Roadmap

Work top to bottom. Tick boxes as they land. Each milestone ends with a
demo you can actually use.

> **Deliberate exception (2026-09-27):** the full `web/` UI — every screen in
> `design_handoff_sentinal/design/`, spanning what would otherwise be Milestones
> 2-4 plus the public landing page — was built up front against a mock data
> adapter (`src/data/mock`, `VITE_DATA=mock`), ahead of the milestones below
> actually landing. This was an explicit, user-approved call so the design
> system and UI shell exist before the backend does; it does not change the
> order daemon/backend work happens in. See `web/FUTURE.md` for what's mock vs.
> real per feature, and `web/QUESTIONS.md` for open questions from the build.

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

## Milestone 1.1 — Single-user authentication
- [x] Persist one Argon2id password hash; bootstrap setup with the machine token
- [x] Issue 24-hour browser session tokens; enforce expiry, logout, and password-change revocation
- [x] Lock password login for five minutes after five failed attempts per client IP
- [x] Keep machine-token authentication for the local CLI
- [x] Wire setup, login, logout, expiry, and password changes through the web adapter
- [x] Protect the authentication HTTP contract with fixtures, daemon tests, Vitest, and Playwright

**Demo:** bootstrap one owner password, sign in from a browser, and use the same
persistent Sessions without exposing the machine token again.

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
