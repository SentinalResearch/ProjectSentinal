# Roadmap

Work top to bottom. Tick boxes as they land. Each milestone ends with a
demo you can actually use.

The product name is **Sentinel**. The target user-facing binaries are
`sentinel` and `sentineld`; the existing `ta` and `tad` names remain in place
until the Milestone 1b migration is implemented and tested.

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
- [x] Shell hooks (zsh, bash, fish) emitting OSC 133
- [x] Auto-source hooks while preserving user shell configuration
- [x] Streaming OSC parser handles arbitrary byte splits and malformed input
- [x] Command tracker records start/end/exit code, cwd, events, and session state
- [x] `store` crate: ordered SQLite migrations, WAL, history, events, and FTS5 search
- [x] Metric retention downsamples raw samples into one-minute averages
- [x] Restart recovery marks orphaned sessions `INTERRUPTED`
- [x] Per-session process tree plus machine CPU/RAM metrics
- [x] Optional NVIDIA GPU metrics behind the `nvidia` feature
- [x] Debounced `RESOURCE_HIGH` events after three consecutive samples
- [x] Stream subscribed session metrics over WebSocket
- [x] Real web history/search and live session resource bars
- [x] Fast tests plus ignored zsh/bash/fish integration coverage

**Demo:** history view with ✓/✗, durations, and live resource usage.

## Milestone 1b — Any terminal into Sentinel (priority, v0.2.x)

This is the next product priority. It is delivered in reviewable stages so
the safety-sensitive shell integration does not land as one large change.
The normative capability and safety rules live in `docs/CAPTURE_MODES.md`.

### 1b.0 — Contract and local ingress
- [x] Add `SessionOrigin`, `CaptureLevel`, and the required Session process identity to the protocol
- [x] Define `CREATE_SESSION` watch fields plus `REGISTER_SHELL`, `SHELL_COMMAND_STARTED`, `SHELL_COMMAND_FINISHED`, and `ADOPT_PROCESS` with hand-reviewed fixtures
- [x] Add capture capability helpers used by daemon and web authorization (daemon enforces now; web consumes them in the capture-aware UI stage)
- [x] Add a private per-user Unix socket with mode `0600` and peer-UID verification
- [x] Reject input/resize/kill for capture levels that do not own a PTY
- [x] Add `docs/SECURITY.md`, `docs/CLI.md`, and `docs/SHELL_HOOKS.md`

### 1b.A — Sentinel Watch
- [ ] `sentinel watch`, `watch -- <command>`, and `watch -t <title> -- <command>`
- [ ] Start in the caller's cwd, set origin `WATCH`, capture `FULL`, and attach immediately
- [ ] Preserve structured argv without evaluating terminal output or concatenating shell text
- [ ] Set and enforce `SENTINEL_SESSION_ID` so Watch cannot nest
- [ ] A closed client only detaches; the Session and process survive
- [ ] Daemon unavailable returns a clear non-zero error and never runs the command unwatched

### 1b.B — External shell hook
- [ ] `sentinel hook zsh|bash|fish`, plus idempotent install and exact uninstall
- [ ] Register ordinary shells as `SHELL_HOOK` / `COMMANDS` Sessions
- [ ] Report Command start, finish, cwd, duration, and exit code through the local socket
- [ ] Inside a FULL Session, continue using OSC and never double-count through the socket
- [ ] Background reporting is silent when unavailable and measures under 5 ms prompt overhead
- [ ] Shell PID liveness ends the reduced-capture Session

### 1b.C — Auto-wrap and Adopt
- [ ] `sentinel auto on|off|status`, off by default
- [ ] Skip IDE task runners, scp/rsync, non-interactive SSH, nested Sessions, and `SENTINEL_DISABLE=1`
- [ ] If the daemon does not answer, auto-wrap falls through to a normal shell within 300 ms
- [ ] `sentinel adopt <pid> [--log <path>] [-t <title>]` and `adopt --pick`
- [ ] Permit same-user processes only; adoption is read-only
- [ ] Detect process exit as “Ended · exit code unknown” and never infer an exit code
- [ ] Detect redirected stdout/stderr off the prompt hot path where the OS supports it

### 1b.D — Adopted log tail and UI completion (lands with Milestone 3)
- [ ] Read-only tail handles append, truncate, rotation, partial lines, and bounded buffering
- [ ] Feed tailed bytes through the deterministic watcher pipeline
- [ ] Upgrade adopted capture from `PROCESS` to `PROCESS_LOG` when a log is active
- [ ] Show a capture badge and one-line limitation on every Session surface
- [ ] Hide attach/input/kill actions when the capture level cannot support them
- [ ] Offer “Watch fully next time” from reduced-capture Sessions

### 1b release gates
- [ ] Fast unit, contract, script, fake-process, and UI capability tests pass
- [ ] Hook latency benchmark records p50/p95/max and p95 is below 5 ms
- [ ] Auto-wrap timeout test proves fail-open within 300 ms without sleeps over 100 ms
- [ ] Real zsh, bash, and fish matrix passes on both Linux and macOS
- [ ] Manual Watch demo survives terminal close and reattaches through CLI and phone
- [ ] Privacy audit confirms no raw keystroke payload is stored on disk

**Demo:** from an ordinary terminal, run `sentinel watch -- python long_job.py`,
close the window, then attach to the same process and screen from another
terminal or phone.

## Milestone 3 — Events + watcher (v0.3)
- [ ] Event engine (raw observations → `Event`)
- [ ] Generic watcher (idle, waiting for input, possibly stalled)
- [ ] Adapters: training, pytest, vLLM
- [ ] Declarative TOML regex adapters
- [ ] Complete Milestone 1b.D adopted-log ingestion and watcher coverage

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
