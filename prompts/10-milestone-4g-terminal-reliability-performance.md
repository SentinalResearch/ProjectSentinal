# MILESTONE 4.G — Real terminal, survives updates, fast and honest status

Goal: a Sentinal terminal behaves like the user's own terminal, survives a
`sentinald` update or crash, never corrupts or silently drops output, and the
dashboard is fast and only says "offline" when a Machine really is.

Background and evidence: `docs/PLAN_R1_TERMINAL_PERFORMANCE.md` (read it
first; every **Found** item there was verified in the code on 2026-10-23).
R1's architecture requirements are in
`prompts/09-reliability-r1-daemon-restart-sessions.md`; this milestone
includes R1 and inherits all of that prompt's design-review, security, and
test requirements.

Already done before this milestone (do not redo): the user's login environment
for Sessions (0.2.4), New session on any Machine, one entry per continued
terminal, and Settings → Diagnostics (browser → Machine and control → worker
round trips plus the Tailscale path, `GET /api/v1/machines/{id}/ping`).

## Owner decisions

Fill these in before pasting. Defaults are the plan's proposals.

| # | Decision | Default |
|---|---|---|
| D1 | Keeper option (A per-Session, B shared, C re-exec, D systemd FD store) | A, as a `sentinald keeper` mode |
| D2 | Output ring per keeper | 1 MiB |
| D3 | Keeper orphan timeout | 24 h |
| D4 | Linux keepers in their own `systemd-run --user --scope` | yes |
| D5 | Dashboard actions allowed on a STALE ("delayed") Machine | yes, routed actions only |
| D6 | Phase order | P0 → R1 → P2 → P3 |

## Rules for this milestone

- Follow `AGENTS.md`: tests first, `just check` before every commit, files
  around 400 lines, product name "Sentinal".
- Work phase by phase in the order D6 gives; commit after each numbered item
  with a focused message. Do not mix R1 code into P0/P2/P3 commits.
- **R1 gate:** before any test or code that moves PTY ownership, write the
  design (section P1.1) to `docs/R1_KEEPER_DESIGN.md`, then stop and ask for
  approval. Do not infer approval from D1.
- New npm packages (xterm addons) and any new crate need a reason in
  `docs/DEPENDENCIES.md`; stop and ask if a crate outside the fixed stack is
  needed.
- Never bind `0.0.0.0`; no credentials in URLs, argv, logs or browser storage;
  terminal bytes never enter logs or audit records.
- Measure before and after every performance change and put the numbers in
  the report. No change justified only by a guess.

## P0 — Small fixes with the most visible effect

1. **Delayed is not offline.** `projection-view.ts` maps STALE to OFFLINE.
   Show STALE as online but delayed ("online · updated 14 s ago", amber) in
   every Machine status surface (sidebar dot, Overview, phone chips, Machines
   tab, Diagnostics). OFFLINE stays the heartbeat verdict (`OFFLINE_AFTER_MS`).
   Per D5, routed actions (attach, input, New session, Recipes) work on a
   delayed Machine. Tests: projection-view mapping, `liveOwner`, and a UI test
   per surface.
2. **Never drop terminal output.** In `crates/daemon/src/api.rs`, an attached
   client that lags the output broadcast logs and continues, losing bytes and
   escape sequences. On `Lagged`, send a fresh snapshot frame (the client
   resets on it, as on attach) and continue from there. Do the same in the
   control proxy path if it can lag. Tests: a fake slow client receives a
   valid screen after lag; no partial-stream continuation.
3. **UTF-8 locale.** If the Session's base environment has no `LANG`/`LC_ALL`,
   read `/etc/default/locale` (Linux) and otherwise set `LANG=C.UTF-8`
   (Linux) or `en_US.UTF-8` (macOS). Unit tests with injected files.
4. **Update warning.** Settings → Machines → Update and `sentinal update` say
   "This ends N live terminals on <Machine>" and need confirmation while
   keepers are not active. Remove the warning in P1 for keeper-backed Sessions.
5. **Load timing.** Add `Server-Timing` to the control's snapshot, registry and
   ping responses, and a `performance.mark` timeline in the PWA shown in
   Settings → Diagnostics ("Last page load: snapshot 120 ms, first Machine
   live 340 ms, …"). This is the baseline for P3.

## P1 — Keepers (R1)

1. **Design and stop.** `docs/R1_KEEPER_DESIGN.md` must answer everything in
   prompt 09's "Required architecture review", using D1–D4, and include:
   - process tree and kill scope on systemd (scope unit) and launchd
     (`setsid` + `AbandonProcessGroup`), and what `sentinal uninstall` does;
   - IPC: socket path and modes, framing, max frame, version handshake,
     peer-UID check, per-keeper secret (stored where, compared how), ring
     replay, backpressure;
   - reclaim sequence after restart, PID-reuse defense, fail-closed cases;
   - measured spike numbers on this Mac and one Linux worker: keeper private
     RSS, added latency per chunk, reclaim time for 10 Sessions.
   Then stop and ask for approval.
2. After approval, implement exactly the approved design behind the existing
   `PtyBackend` trait (`FakePty` tests keep working; `RealPty` stays as the
   fallback when keepers cannot start, and the Session says so).
3. Startup reclaim replaces `interrupt_orphaned_sessions` for keeper-backed
   Sessions; every mismatch marks the Session INTERRUPTED with a structured
   reason the UI shows ("Stopped: Sentinal restarted and could not reclaim
   this terminal").
4. Installer, `sentinal setup`, and service files gain the approved kill-scope
   changes; upgrade from 0.2.x keeps working (old Sessions are interrupted
   once, new ones are keeper-backed).
5. Acceptance (real hardware, recorded in the report): on this Mac and on one
   Linux worker, run `top`, `vim`, and a long build in three Sessions; update
   the worker from the dashboard; all three continue with intact screens and
   complete logs. Hard-kill `sentinald` and repeat. Kill one keeper: only that
   Session ends.

## P2 — "It feels like my own terminal"

1. **Scrollback on attach.** Attach sends the last N scrollback rows
   (formatted, default 2 000) before the screen, so scrolling up in the
   browser shows earlier output. Lower the daemon's vt100 history to that N
   (today 10 000 rows ≈ 38 MB per Session at 120 columns, never sent). Report
   daemon RSS before/after with 10 busy Sessions.
2. **bash like SSH.** Dashboard bash behaves like an SSH login shell: the
   user's `~/.bash_profile` (or `~/.bash_login`/`~/.profile`) runs, and our
   hook still loads exactly once. Same check for zsh `.zprofile`/`.zlogin` and
   fish. Integration test per shell (`#[ignore]`, real shells).
3. **xterm.js addons** (same project, justify in `docs/DEPENDENCIES.md`):
   `unicode11` (emoji/CJK width), `web-links` (clickable, `noopener`),
   `clipboard` (OSC 52 copy with a one-time permission prompt).
4. **Fonts and keys.** Settings → Terminal: font (default JetBrains Mono, plus
   a Nerd Font option for prompt glyphs), font size, Option-as-Meta on macOS,
   browser scrollback (default 10 000). Stored per browser in `localStorage`
   (preferences only, never secrets).
5. **Title and bell.** OSC 0/2 title shows in the Session header and browser
   tab; bell flashes the pane.
6. **Torture test.** A Playwright spec against a real daemon: vim, htop,
   tmux inside a Session, `cat` of a 50 MB file over a throttled socket (no
   corruption: the final screen equals the daemon's screen), emoji/CJK file,
   resize storm, OSC 52 copy, a clickable URL.

## P3 — Performance and resilience from measurements

1. Use the P0.5 timings and Diagnostics on the owner's real Tailnet
   (the real control and its workers) to find the slow step; fix the top
   two causes. Expected candidates, in order: relayed Tailscale paths (surface
   them; document UDP 41641), first-load snapshot size, per-worker proxy
   setup, one WebSocket per Split View pane.
2. **Adaptive metrics.** Workers sample every 2 s and upload every second even
   when nobody is watching. Sample every 2 s only while a dashboard is open
   (the control knows); otherwise every 15–30 s. Keep projection freshness
   correct (a heartbeat-only refresh must keep a quiet Machine LIVE).
3. **Fast reconnect.** The PWA reconnects at once on `online` and
   `visibilitychange`, not after the backoff timer; the worker link re-tries
   at once after a network change.
4. **`sentinal doctor`.** Checks version, control URL, link state, service
   state, leftover rc blocks, Tailscale path to the control, and one timed
   round trip; prints the exact fix for each failure. `--json` for support.
5. Multi-viewer size: the PTY follows the smallest active viewer, with a
   "this view controls the size" toggle.

## Tests first

Every numbered item starts with a failing test (unit, integration, or
Playwright). Fast tests have no sleeps over 100 ms; real shells, services, and
processes stay in `#[ignore]` tests listed in the report. Do not weaken or
delete existing tests; the e2e chain in `web/playwright.config.mjs` keeps
`projection-acceptance` last.

## Done when

- P0, P1 (after approval), P2, and P3 items are committed, `just check` and
  `just test-e2e` pass.
- The report contains: before/after numbers for load time, daemon RSS, keeper
  RSS and latency; real-hardware results for P1.5 on macOS and Linux; the
  Diagnostics table from the owner's Tailnet; anything not done and why.
- `docs/ROADMAP.md` R1 items are checked; `docs/PLAN.md` no longer says a
  daemon restart interrupts FULL Sessions (except reboot/logout/RealPty
  fallback, stated precisely); `docs/AUDIT_TRACKER.md` F1 updated.
- Stop with the report and "Questions for you". Do not tag or push.
