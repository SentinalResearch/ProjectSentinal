# TERMINAL UPGRADE PACKAGE — start anywhere, pick up anywhere, one clean screen

Goal: start work in a terminal on one Machine and pick it up from any other
terminal, phone, or browser with one command, `sentinal watch`, and always see
one correct screen. No tmux, no second tool: one common path for text in and
text out.

This package replaces the open parts of Milestone 4.G and Reliability R1. It
is built in six stages, T0 to T5. Each stage is one change set that passes
`just check` on its own. Do the stages in order; do not start a stage before
the one before it is reported and reviewed.

Read before changing code: `AGENTS.md`, `docs/ARCHITECTURE.md`,
`docs/TESTING.md`, `docs/TASKS.md` (section **T**), `docs/R1_KEEPER_DESIGN.md`,
`docs/CAPTURE_MODES.md`, `docs/SECURITY.md`, and
`prompts/10-milestone-4g-terminal-reliability-performance.md`.

Hard rules from `docs/TASKS.md` apply unchanged: never touch production, test
only with `just dev`, no commit, push, tag, or release unless the owner asks,
no commands on the owner's VMs.

## Owner decisions (2026-11-12, do not reopen)

1. **One size per Session, set by the latest active viewer.** The viewer the
   owner last typed in or focused sets the Session's size. Every other viewer
   shows the same screen at that size, scaled, cropped, or letterboxed. A
   viewer that only looks never changes the size.
2. **Terminal engine: `libghostty-vt`** replaces the `vt100` crate as the
   daemon's screen state, subject to the build gate in T2. It is recorded in
   `docs/DEPENDENCIES.md`.
3. **In scope:** display fixes, the keeper (Sessions survive a daemon restart
   or update), the watcher fixes listed in T0, and `sentinal watch` as the one
   pick-up command.
4. **Out of scope:** tmux or any other multiplexer bridge; taking over a
   process that was not started under Sentinal (`adopt` stays read-only);
   models; new dashboard pages.

## What is wrong today (found by reading and running the code)

Display:

- Every viewer sends `RESIZE` and the PTY takes whichever came last
  (`web/src/terminal/useRealTerminalSession.ts`, `crates/cli/src/client.rs`).
  The other viewers keep drawing raw bytes at their own size.
- The attach snapshot is taken before the program has redrawn for the new
  size; the CLI writes the snapshot and only then resizes.
- A viewer can join, or be redrawn after falling behind, in the middle of an
  escape sequence or a UTF-8 character
  (`crates/daemon/src/session_manager/operations.rs`, `ScreenSource`).
- The web terminal loads no Unicode width addon, so emoji and wide symbols
  take a different number of cells than the program assumed.
- `fit.fit()` runs on every ResizeObserver tick with no debounce.
- The CLI has no SIGWINCH handling and does not restore terminal modes on
  detach.
- `web/src/terminal/peek.ts` renders at a fixed 120×24 whatever the PTY size.
- Any `0x0A` byte detaches a CLI viewer and drops the rest of that input
  (`crates/cli/src/detach.rs`).
- `sentinal detach <id>` waits for a reply the daemon never sends, and cannot
  detach another terminal anyway (`crates/cli/src/app.rs`, uncommitted).

Continuity:

- FULL Sessions end when the daemon restarts or updates
  (`store.interrupt_orphaned_sessions`).
- There is no single command that lists what is running on every Machine and
  attaches to it.

Watcher: the T0 list below.

## Target design

```
program ⇄ PTY ⇄ keeper (one per Session, owns PTY + log)
                  ⇅ private socket
                daemon: authoritative screen (libghostty-vt), size policy,
                        watcher, viewers
                  ⇅ WebSocket (direct, or relayed by the control)
        web terminal · phone · `sentinal watch`
```

Rules of the design:

1. **The daemon owns the screen.** One engine instance per Session, at the
   Session's size. The watcher reads questions and progress from this screen,
   never from a viewer.
2. **Viewers never resize the PTY directly.** A viewer reports its own size
   and whether it is active. The daemon decides the Session size and tells
   every viewer.
3. **A viewer always starts from a whole screen.** The first frame a viewer
   gets is a snapshot produced by the engine at the Session size, followed by
   bytes that start on a sequence boundary. The same holds for a redraw after
   lag and for every size change.
4. **Viewers render at the Session size.** The web terminal sets its rows and
   columns to the Session size and scales or pans to fit its box. The CLI
   crops or pads. Only becoming the active viewer changes the size.
5. **Fail open.** If the engine, keeper, or relay is unavailable, the Session
   keeps running and the owner can still type; a viewer shows why it is
   degraded.

## T0 — Watcher and daemon fixes (no protocol change except where noted)

Each item: a test that fails first, then the fix.

- [ ] T0.1 `crates/protocol/src/evidence.rs`: `supports_evidence` returns
      true for `AgentActivity` and `AgentTurnFinished` wherever terminal
      output is observable. Test through `EventEngine`, not the adapter.
- [ ] T0.2 `crates/watcher/src/training.rs:282`: compare bytes, never slice a
      string at a byte offset. Test with `🚀 loss: 0.5` and `•  loss=0.5`.
      Add a property-style test that feeds mixed-width text to every adapter.
- [ ] T0.3 `crates/daemon/src/session_manager/tasks.rs`: a panic inside the
      watcher must not stop the PTY reader. Catch it, log once, replace the
      engine with a fresh one, keep reading.
- [ ] T0.4 `crates/watcher/src/generic.rs` `server_started_line`: take the
      port after the last `:` of a host:port or the word `port`; report a
      server once per Command. Tests: `127.0.0.1:8080`, `10.0.0.5:3000`, a
      line starting with a timestamp, `[::]:3000`.
- [ ] T0.5 `agent_cli.rs` / `agent_activity.rs`: count a tool line once while
      it stays on screen (key by row content across turns, clear when it
      scrolls off); pass the screen's waiting state into `turn.tick`.
- [ ] T0.6 `health.rs`: a `WaitingForInput` warning ends when the question is
      answered; an error line while the Command still runs is `WARNING`, and
      only a failed exit is `FAILED`.
- [ ] T0.7 `engine.rs` / `progress.rs`: rate-limit `MetricReported` per name
      (same interval as progress); bound `unit` to 32 characters; a
      `::sentinal` line with any malformed field is ignored whole.
- [ ] T0.8 `screen_prompt.rs`: a question that leaves the screen and returns
      is a new question, even with the same text. Cap marked menus at
      `MAX_PROMPT_OPTIONS`. Report which row is marked (see T3.4).
- [ ] T0.9 Notifications: a new `WaitingForInput` Event is never
      deduplicated against an earlier one once that earlier question was
      answered (`daemon/src/notifications.rs`, `store/src/native_push.rs`).
- [ ] T0.10 `session_manager/operations.rs` `kill`: finish the current
      Command, broadcast `SESSION_UPDATED`. The same for Sessions interrupted
      at startup.
- [ ] T0.11 Rate-limit `WarningDetected` and `ErrorDetected` per Session
      (first of a burst, then a count), so a noisy build cannot flood the
      store or slow the reader.
- [ ] T0.12 `store`: decoding one unreadable Session or Event row skips that
      row and logs it; the query still returns the rest. Test with an Event
      kind and a `CodingAgent` value this build does not know.
- [ ] T0.13 Projection: do not compact a row at or below the cursor that is
      being uploaded; cap a snapshot to the newest records that fit the
      control's chunk limit and say so in the batch
      (`store/src/projection.rs`, `daemon/src/link/mod.rs`). Tests for both.

## T1 — Display fixes that need no new protocol

- [ ] T1.1 Web: add `@xterm/addon-unicode11` (record it in
      `docs/DEPENDENCIES.md`), activate version 11. Browser test with emoji
      and CJK lines checks column alignment.
- [ ] T1.2 Web: debounce fit (one resize per 100 ms, trailing).
- [ ] T1.3 CLI: handle SIGWINCH and send the new size; on any exit from
      attach, restore the local terminal (leave alternate screen, show
      cursor, mouse reporting off, bracketed paste off, reset attributes).
- [ ] T1.4 CLI: resize before asking for the snapshot.
- [ ] T1.5 Daemon: never start a viewer's stream inside a sequence. Until T2
      lands, hold back the bytes after the last complete sequence when a
      snapshot is taken (`ScreenSource`), for attach and for lag redraw.
      Test: attach at every byte offset of a recorded stream; the viewer's
      final screen must equal the daemon's.
- [ ] T1.6 CLI detach key: the default becomes `Ctrl-\ d` only. `Ctrl+J`
      passes through to the program. Remove the `detach` subcommand added on
      `dev`, or make it close this Machine's own attached clients and return.
      Ask the owner which before coding.

## T2 — Authoritative screen on `libghostty-vt` (design review first)

### Build gate — stop and report before any other T2 work

`libghostty-vt` is pre-1.0 and builds a C library with Zig. Prove, in a
throwaway branch, on the four release targets
(`aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`):

1. it builds in CI without network access at build time (vendored source or a
   pinned Zig toolchain recorded in the release workflow);
2. binary size and daemon start time before and after;
3. a throughput test: 100 MB of mixed output through the engine on its own
   thread, compared with `vt100`;
4. it runs every recorded screen in `crates/watcher/tests/` and
   `crates/daemon/tests/` and produces the same visible text as `vt100`, with
   a written list of differences.

Report the numbers and stop. If any target cannot build reproducibly, the
fallback is to keep `vt100` behind the same module and continue with T3; the
owner decides.

### Work after the gate

- [ ] T2.1 One module, `crates/daemon/src/screen/`, is the only code that
      names the engine. It offers: feed bytes, resize with reflow, snapshot
      as bytes for a viewer, rows of text for the watcher, alternate-screen
      flag, cursor, and "at a sequence boundary". `unsafe_code = "forbid"`
      stays; no `unsafe` in this workspace.
- [ ] T2.2 The engine runs on one dedicated thread per Session (it is not
      thread-safe); the PTY reader sends it bytes over a bounded channel and
      never blocks on a slow viewer.
- [ ] T2.3 `sample_screen`, attach snapshot, lag redraw, `scrollback`, and
      output replay all read from this module. Delete the `vt100` dependency
      when nothing uses it.
- [ ] T2.4 Scrollback: the snapshot a viewer gets includes the last N lines
      of history (default 2,000, from `daemon.toml`), so picking up a Session
      shows what happened before, not only the visible screen.

## T3 — One size per Session, and viewers that follow it (protocol change)

Present the wire proposal and fixtures first; wait for approval.

Proposal to refine:

- `ATTACH` gains optional fields, all defaulted for old clients:
  `cols`, `rows`, `mode` (`INTERACTIVE` or `OBSERVE`).
- New client message `VIEWER_STATE { session_id, cols, rows, active }`,
  sent when the viewer's box changes or it gains or loses focus.
- New daemon message `SESSION_SIZE { session_id, cols, rows, owner }`, sent
  to every viewer when the Session size changes, immediately followed by a
  fresh snapshot frame. `owner` says whether this viewer set it.
- `RESIZE` from an old client is treated as `VIEWER_STATE` with
  `active = true`, so old CLIs and cached PWAs keep working.
- Size policy in the daemon, not in clients:
  - the active viewer is the one that last sent input or `active = true`;
  - the Session takes its size after 150 ms without a newer report;
  - when the active viewer leaves, the size stays until another viewer
    becomes active;
  - `OBSERVE` viewers (peek cards, read-only pages) never become active;
  - bounds: 20–500 columns, 5–200 rows.
- Every accepted size change is recorded in the audit log with the client
  address, like input.

Work:

- [ ] T3.1 Protocol types, fixtures, generated TypeScript; say so in the
      commit message and under "Release notes" in `docs/TASKS.md`.
- [ ] T3.2 Daemon size policy with unit tests driven by an injected clock:
      two viewers alternating, a viewer leaving, an observer, an old client.
- [ ] T3.3 Web terminal renders at the Session size: when it is not the
      owner it scales the font down to fit the width, never below a readable
      minimum, then pans; a one-line bar says "Sized for <other device> ·
      Use this size" and one tap makes this viewer active. Peek cards attach
      as `OBSERVE` and render at the Session size.
- [ ] T3.4 Answer buttons use the marked row reported by the watcher: the
      keys for a marked menu are computed in the daemon from the current
      screen at the moment of answering (move from the marked row to the
      chosen row, then Enter), and refused if the question on screen is no
      longer the one that was shown. Remove the Vibe-only arrow logic from
      `web/src/data/actions.ts`.
- [ ] T3.5 CLI attach follows `SESSION_SIZE`: typing makes it active; when
      it is not the owner it crops to its window and shows one status line.
- [ ] T3.6 Browser test: two pages of different sizes on one Session, type
      in each in turn, both screens stay identical cell for cell.

## T4 — `sentinal watch`: start anywhere, pick up anywhere

Present the command design first; wait for approval.

Proposal to refine:

```
sentinal watch -- COMMAND…        start COMMAND in a new Session and attach
sentinal watch                     pick up: list live Sessions, choose one
sentinal watch NAME|ID             pick up that Session
sentinal watch --last              pick up the Session most recently active
sentinal watch --new               start a new shell Session (today's behaviour)
sentinal watch --machine NAME …    limit to one Machine
```

- With no argument, on a terminal: live Sessions on every Machine this
  Machine can see, the ones waiting for the owner first, each with Machine,
  name, state, and running time. One Session: attach to it directly. None:
  start a new shell Session, as today.
- With no terminal (a script or an agent): never prompt; print the list and
  exit non-zero unless `--last`, an ID, or a COMMAND was given.
- A Session on another Machine is reached through the control, with the same
  routing and credentials the browser uses; the worker still enforces
  capture and audit. If this Machine is not linked to a control, only local
  Sessions are listed and the output says so.
- `NAME` matches the Session title or its agent name and folder; an
  ambiguous name lists the matches and exits non-zero.
- On attach the viewer gets the T2.4 history and then the live screen, so
  the owner sees what happened while away.

Work:

- [ ] T4.1 Listing and matching as pure functions with tests.
- [ ] T4.2 CLI routing to a remote Session through the control; tests with
      two in-process daemons.
- [ ] T4.3 `docs/CLI.md`, `docs/AGENT_GUIDE.md`, `sentinal watch --help`.
- [ ] T4.4 Tried by hand in `just dev`: start `claude` in one terminal,
      close it, run `sentinal watch` in another, answer a question from the
      phone layout, return to the first terminal.

## T5 — Keeper: Sessions survive a daemon restart or update

Follow `docs/R1_KEEPER_DESIGN.md`. Its section 13 lists questions for the
owner; present them with your recommended answers and wait.

- [ ] T5.1 Keeper process per FULL Session owns the PTY and the saved-output
      log; the daemon is its client over a private owner-only socket.
- [ ] T5.2 At startup the daemon reclaims its own keepers, replays the saved
      output through a fresh engine to rebuild the screen (T2), and resumes
      the watcher from the tracker state the keeper kept. A keeper it cannot
      verify is left alone and reported, never adopted.
- [ ] T5.3 `store.interrupt_orphaned_sessions` interrupts only Sessions with
      no live keeper.
- [ ] T5.4 Update paths (`sentinal update`, worker update from the
      dashboard, control self-update) no longer count live FULL Sessions as
      interrupted; the "this will interrupt N Sessions" confirmation goes
      away when keepers are on.
- [ ] T5.5 Tests: kill the daemon mid-output and restart; update with a
      Session running a full-screen program; keeper crash; keeper from an
      older version.

## Tests first

For each stage, write the failing tests and show them failing before the
change. Recorded real screens beat invented ones; add fixtures under the
crate's `tests/` directory. Browser tests use the fake PTY; say plainly which
behaviour was only tried by hand.

## Done when

- `just check` and `just test-e2e` pass; the real output is in the report.
- Two viewers of different sizes on one Session show identical screens in the
  browser test and in the two-terminal manual test.
- `sentinal watch` in a second terminal picks up a Session started in the
  first, shows its history, and typing works.
- Restarting the dev daemon leaves a running Session running, and a viewer
  reconnects to the same screen.
- Every T0 item has a test that failed before its fix.
- `docs/TASKS.md` section T and "Release notes", `docs/DEPENDENCIES.md`,
  `docs/CLI.md`, and `docs/progress/terminal-upgrade.md` are updated.

## Report after each stage

What changed (files), tests added, the real output of `just check` (and
`just test-e2e` when the web changed), what was tried by hand in `just dev`
and what was seen, and anything not verified. Then stop.
