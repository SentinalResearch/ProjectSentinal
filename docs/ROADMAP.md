# Roadmap

Work top to bottom. Tick boxes as they land. Each milestone ends with a
demo you can actually use.

The product name is **Sentinal**. The target user-facing binaries are
`sentinal` and `sentinald`; the existing `ta` and `tad` names remain in place
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

## Milestone 1b — Any terminal into Sentinal (priority, v0.2.x)

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

### 1b.A — Sentinal Watch
- [x] `sentinal watch`, `watch -- <command>`, and `watch -t <title> -- <command>`
- [x] Start in the caller's cwd, set origin `WATCH`, capture `FULL`, and attach immediately
- [x] Preserve structured argv without evaluating terminal output or concatenating shell text
- [x] Set and enforce `SENTINAL_SESSION_ID` so Watch cannot nest
- [x] A closed client only detaches; the Session and process survive
- [x] Daemon unavailable returns a clear non-zero error and never runs the command unwatched

### 1b.B — External shell hook
- [x] `sentinal hook zsh|bash|fish`, plus idempotent install and exact uninstall
- [x] Register ordinary shells as `SHELL_HOOK` / `COMMANDS` Sessions
- [x] Report Command start, finish, cwd, duration, and exit code through the local socket
- [x] Inside a FULL Session, continue using OSC and never double-count through the socket
- [x] Background reporting is silent when unavailable and measures under 5 ms prompt overhead
- [x] Shell PID liveness ends the reduced-capture Session

### 1b.C — Auto-wrap and Adopt
- [x] `sentinal auto on|off|status`, off by default
- [x] Skip IDE task runners, scp/rsync, non-interactive SSH, nested Sessions, and `SENTINAL_DISABLE=1`
- [x] If the daemon does not answer, auto-wrap falls through to a normal shell within 300 ms
- [x] `sentinal adopt <pid> [--log <path>] [-t <title>]` and `adopt --pick`
- [x] Permit same-user processes only; adoption is read-only
- [x] Detect process exit as “Ended · exit code unknown” and never infer an exit code
- [x] Detect redirected stdout/stderr off the prompt hot path where the OS supports it

### 1b.D — Adopted log tail and UI completion (lands with Milestone 3)
- [x] Read-only tail handles append, truncate, rotation, partial lines, and bounded buffering
- [x] Feed tailed bytes through the deterministic watcher pipeline
- [x] Upgrade adopted capture from `PROCESS` to `PROCESS_LOG` when a log is active
- [x] Show a capture badge and one-line limitation on every Session surface
- [x] Hide attach/input/kill actions when the capture level cannot support them
- [x] Offer “Watch fully next time” from reduced-capture Sessions

### 1b release gates
- [x] Real daemon data is the default; mock fixtures require explicit demo mode
- [x] One-command Tailscale-only development launcher starts daemon and web UI together
- [x] Explicit mock-only user-testing scenarios are repeatable, resettable, and unavailable in real API mode
- [x] Fast unit, contract, script, fake-process, and UI capability tests pass
- [x] Hook latency benchmark records p50/p95/max and p95 is below 5 ms
- [x] Auto-wrap timeout test proves fail-open within 300 ms without sleeps over 100 ms
- [x] Real zsh, bash, and fish matrix passes on both Linux and macOS (macOS 2026-10-01, Linux pop-os 2026-10-03)
- [x] Manual Watch demo survives terminal close and reattaches through CLI and phone
- [x] Privacy audit confirms no raw keystroke payload is stored on disk

**Demo:** from an ordinary terminal, run `sentinal watch -- python long_job.py`,
close the window, then attach to the same process and screen from another
terminal or phone.

### 1b.E — Finished Session lifecycle and output replay
- [x] Finished FULL Sessions remain viewable but reject input, resize, signals, and kill before touching a closed PTY
- [x] Web terminal becomes visibly read-only for `COMPLETED`, `FAILED`, and `INTERRUPTED`
- [x] “Continue in new Session” reuses safe metadata but never claims to resurrect the old process
- [x] Persisted `.log.zst` output is available through a bounded authenticated replay/export path
- [x] Errors name the Session and terminal state instead of exposing an OS `EIO`
- [x] Regression coverage proves rejected input is not written or input-audited

**Demo:** open a completed Session, inspect its saved output without PTY errors,
then continue in a new FULL Session with the same cwd.

## Milestone 1c — Deployable worker and two-host acceptance
- [x] `just agent` runs a daemon-only worker on its exact Tailscale address in real API mode
- [x] `just control WORKER_IP=...` runs the web UI against one remote worker without starting a local daemon
- [x] A release installer installs `sentinal`, `sentinald`, shell assets, safe config, and the existing user service
- [x] Upgrade and uninstall preserve user data and credentials unless deletion is explicitly requested
- [x] `docs/SETUP.md` documents macOS/Linux, Tailscale ACLs, service inspection, backup, and recovery
- [x] A two-host manual checklist proves Watch → detach → browser attach → exit/kill → SQLite/log/audit
- [x] The worker package remains the daemon + CLI + hooks; no duplicate “watcher agent” process is introduced

**Demo:** install the worker on one Machine, run only the control UI on a
second Machine, and complete one real Watch handoff over Tailscale.

## Milestone 1c.1 — Tailnet bootstrap and private package delivery
- [x] Approve the package-service, release-manifest, HTTPS, and integrity design before implementation
- [x] Build versioned Linux/macOS worker bundles for supported architectures from the existing worker package
- [x] Serve a bounded release manifest, immutable artifacts, and `install.sh` from an owner-controlled Tailnet control Machine
- [x] Verify target, length, compatibility, and checksum before invoking the existing installer
- [x] Installation never enrolls or receives a pairing secret; it ends visibly `UNPAIRED`
- [x] Reinstall/update preserve config, credentials, Machine data, SQLite, logs, adapters, and recipes
- [x] `sentinal doctor` reports install/service/Tailscale health without connecting implicitly
- [x] Prove curl install on an available real peer and retain the offline/source-tree install path

**Demo:** copy one Tailnet HTTPS curl command onto a fresh Linux Machine,
install the worker, and see `UNPAIRED` plus the separate `sentinal connect`
next step.

## Milestone 1d — Session continuation history
- [x] Approve the additive lineage wire/storage proposal before implementation
- [x] Persist a linear predecessor/successor relationship across browser and daemon restart
- [x] Continue a finished FULL Session atomically into one distinct UI/FULL Session generation
- [x] Preserve each generation's immutable output, Commands, exit result, Metrics, and audit boundary
- [x] Present bounded combined output with visible generation boundaries and version navigation
- [x] Route input, resize, signals, and kill only to the latest live generation
- [x] Prove duplicate/concurrent Continue requests create exactly one successor

**Demo:** finish a Session, Continue it twice across page reloads, inspect all
three retained outputs as one ordered history, and interact only with the
latest live generation.

## Milestone 3 — Events + watcher (v0.3)
- [x] Event engine (raw observations → persisted and live `Event` values)
- [x] Generic watcher (waiting for input, possibly stalled, errors, warnings)
- [x] Adapters: training, pytest, vLLM/llama.cpp, and Cargo
- [x] Declarative TOML regex adapters with bounded deterministic loading
- [x] Deterministic `Health` contract, persistence, evidence, and web card
- [x] Bounded persisted Event replay and real web Event timeline/progress
- [x] Saved-log snapshots, rate-limit tests, and synthetic Health/resource coverage
- [x] Complete Milestone 1b.D adopted-log ingestion and watcher coverage

## Milestone 4.0 — Secure Machine enrollment
- [x] Complete the mandatory pairing threat-model and wire-format review before implementation
- [x] Generate short-lived, single-use pairing authority without exposing the Machine token to a browser or control service
- [x] Persist an immutable Machine ID separately from its owner-editable display name
- [x] `sentinal connect` pairs one installed worker through a hidden prompt/stdin and never accepts a pairing code in argv or a URL
- [x] Exchange the one-time code for a unique revocable Machine credential; the Machine token stays local
- [x] Persist the owner-controlled Machine registry, connection state, last seen, and per-Machine credential metadata locally
- [x] Rename, reconnect, disconnect, and revoke one Machine without reinstalling it or affecting another
- [x] Audit pairing creation, exchange, expiry, revocation, and failed attempts without logging secrets
- [x] Document HTTPS/Tailscale Serve and explicit allowed browser origins

**Demo:** install a worker, choose Add Machine in the control UI, run
`sentinal connect CONTROL_URL --name "RTX PC"`, enter the code at the hidden
prompt, and see the stable Machine ID appear under that display name.

## Milestone 4.A — Multi-Machine aggregation
- [ ] The local control Machine connects independently to every enrolled daemon; there is no required cloud service
- [ ] Namespace Session/Command/Event identities by Machine in the web data layer
- [ ] Isolate failures and show offline Machines with last-seen time
- [ ] Route attach, input, Metrics, history, and search to the owning Machine only
- [ ] Prove two fake daemons in Playwright, then complete a two-real-Machine acceptance run

## Milestone 4.B — Recipes + notifications + mobile dashboard (v0.4)
- [ ] Recipe files in `recipes/`, launch from UI
- [ ] "Save as recipe" after a successful command
- [ ] ntfy notifications for completed / failed / waiting / stalled

## Reliability R1 — Daemon-restart-resilient FULL Sessions
- [ ] Approve a keeper-process architecture and shutdown/upgrade semantics before implementation
- [ ] A per-Session owner keeps the PTY and log alive while `sentinald` restarts
- [ ] A restarted daemon securely reclaims same-user keepers and rebuilds screen/tracker state
- [ ] Missing, foreign, or inconsistent keepers fail closed and mark the Session interrupted
- [ ] Service upgrade and crash tests prove the process, screen, log, and audit boundary survive

## V2 — Intelligence
- [ ] Tool API for the agent (same permissions as a user, capped at Level 2)
- [ ] Eval harness with hand-labelled sets
- [ ] Fine-tune small model (FunctionGemma 270M / Qwen3-0.6B) for
      NL → tool call routing, then command classification, then triage
- [ ] Heartbeat, journal, summaries
