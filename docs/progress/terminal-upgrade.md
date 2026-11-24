# Terminal upgrade: progress

Prompt: `prompts/13-terminal-upgrade-package.md`. Tasks: `docs/TASKS.md`
section T. Point-in-time evidence, as the other reports in this directory.

## T2: authoritative screen on libghostty-vt (2026-11-22)

Tested on a working tree based on commit `13781d8`, macOS 27.0 (Apple
silicon, Mac mini), rustc 1.97.1, Zig 0.17.1 (Homebrew).

### Build gate

The gate ran in a throwaway crate outside the repository, with the crate the
daemon now uses: `ghostty-vt` `=0.9.0` (libghostty-vt release 1.0.1).

| Target | Result |
|---|---|
| aarch64-apple-darwin | Builds and runs; links the release's prebuilt XCFramework (sha256-pinned), no Zig |
| x86_64-apple-darwin | Builds and links; not run (no Rosetta on the test machine). Intel Macs were dropped afterwards (owner, 2026-11-22) |
| x86_64-unknown-linux-musl | Builds a static binary from the pinned source with Zig 0.17.1; not run (no Linux machine or working Docker) |
| aarch64-unknown-linux-musl | Same |

Offline: the Linux source build reads the source and dependency tarballs
named in the crate's `GHOSTTY.lock`; `zig build --system` then fetches
nothing. `scripts/fetch-terminal-engine.sh` fetches those files (and Zig on
Linux) before the release build, pinned by sha256, and the build runs with
`--offline`. Cross-building Linux from a Mac needs Zig as the linker with
`-C link-self-contained=no`; the release workflow builds Linux on Linux.

| Measure | vt100 0.16 | libghostty-vt 1.0.1 |
|---|---|---|
| 100 MB mixed output, 8 KiB chunks, one thread | 0.42 s (237 MB/s) | 0.18 s (562 MB/s) |
| Stripped binary, macOS arm64 (minimal program) | 393 KB | 1.6 MB |
| Stripped static binary, Linux x86_64 | 433 KB | 2.8 MB |
| Stripped static binary, Linux aarch64 | 408 KB | 2.4 MB |
| Start and exit, mean of 200 runs | 2.72 ms | 2.83 ms |
| Create a 120×40 terminal | n/a | 7.5 µs |

Parity: 119 streams (the 9 recorded logs in `crates/watcher/tests/logs`, 96
escape-bearing strings from the watcher and daemon tests, 14 hand-made
streams) through both engines at 80×24. 117 identical. The two differences:

1. Tab stops set with `ESC H` after `ESC[3g`: vt100 puts the next character
   one column early; libghostty-vt follows the stops.
2. DEC line drawing (`ESC(0`): vt100 shows `lqqk`; libghostty-vt draws `┌──┐`.

Both are vt100 bugs. The owner chose libghostty-vt.

### What was built

| Part | Where |
|---|---|
| The engine, the only code that names it: feed (query answers and other effects dropped), resize with reflow, snapshot, rows, alternate flag, cursor, text lines | `crates/daemon/src/screen/engine.rs` |
| One screen thread per Session; bounded feed channel (64 reads); draws, then publishes; snapshots answered between feeds with a receiver subscribed at that moment; fail open without an engine | `crates/daemon/src/screen/mod.rs` |
| Snapshot on the alternate screen: the main screen first (from a copy of the engine that leaves the alternate screen), then the alternate screen, so a viewer has the main screen back when the program leaves | `engine.rs`, `snapshot` |
| History in the snapshot: about `attach_history_lines` (`daemon.toml`, default 2,000); a second copy asks for more when libghostty's page-sized trimming left under 90 % | `engine.rs`, `config.rs`, `entry.rs` |
| Reader, watcher rows, attach, lag redraw, resize and `scrollback` moved to the screen; `vt100`, `screen_history.rs` and the line ring on each Session are gone | `session_manager/{tasks,operations,lifecycle,external}.rs`, `api/attach.rs` |
| Release workflow fetches the engine's files first | `.github/workflows/release.yml`, `scripts/fetch-terminal-engine.sh` |

`SequenceBoundary` (T1.5) stays: libghostty-vt cannot carry an unfinished
UTF-8 character into a snapshot.

### Tests

New in `crates/daemon/src/screen/tests.rs`: output drawn then published in
order; a snapshot and its receiver split the output exactly; leaving the
alternate screen after an attach brings back the main screen (failed before:
the T1.5 finding); a viewer on the alternate screen sees the program; history
snapshot carries about the configured lines and a plain one none; colours and
cursor survive; resize reflows; watcher rows and flag; queries neither
answered nor piling up; fail open without an engine; a viewer attaching at
every byte of a recorded stream ends with the daemon's screen; three recorded
logs draw as the program meant (`\r` progress overwritten in place). Moved off
vt100: the ScreenSource, lag-redraw and attach integration tests. Config test
for `attach_history_lines`.

`just check`: passed. Rust 1,438 passed, 0 failed, 22 ignored; web 586
passed (107 files); script suites passed.

`just test-e2e`: 20 passed (2.4 min).

### Tried by hand (`just dev` QA instance, web on 127.0.0.1:5283)

A Session printed 40 coloured lines, opened a full-screen view, and closed it
75 s later. The web terminal opened during the full-screen view showed the
monitor with its reverse-video title. When the program left, that viewer
showed the 40 lines and "back on the main screen" (before T2 it would have
shown only the last line). A reload showed the history, scrollable to the
start. The bytes the daemon sent were checked over the WebSocket: each
history line resets its style. The reloaded history looked slightly heavier
than live output in the browser; the bytes carry no bold, so this is
how the browser draws it, not the daemon's output.

### Not verified

- Linux binaries were built, not run.
- `scripts/fetch-terminal-engine.sh` was checked with shellcheck, not run
  (it downloads release files); the workflow change has not run in CI.
- Linking the Zig-built library with the musl toolchain CI uses
  (`musl-tools`) was not tried; locally Zig was the linker.
- The CLI still attaches without history; that is T4.

## T3: one size per Session (2026-11-22)

Design: `docs/TERMINAL_SIZE_DESIGN.md`, approved by the owner on 2026-11-22
with the four recommended decisions. Tested on a working tree based on commit
`050572e`.

### What was built

| Part | Where |
|---|---|
| `ATTACH.cols`, `rows`, `mode`; `VIEWER_STATE`; `ANSWER_PROMPT`; `SESSION_SIZE`; `ViewerMode`; fixtures and TypeScript | `crates/protocol/src/message.rs`, `tests/fixtures/`, `web/src/protocol/` |
| Size policy: pure, clock injected | `crates/daemon/src/session_manager/viewers.rs` |
| Viewers attached, reports, typing, old `RESIZE`, settle timer, audit, PTY and screen resize | `session_manager/viewer_ops.rs`, `audit.rs` |
| A size change: `SESSION_SIZE` to sized viewers, then a reset and a snapshot | `api/attach.rs`, `api.rs` |
| Menu answers from the current screen | `session_manager/answer.rs`, `watcher::menu_on_screen` |
| Web: attach with the box, draw at the Session size, shrink to fit, pan, bar | `web/src/terminal/{useRealTerminalSession.ts,TerminalPane.tsx,sessionSize.ts}` |
| Peek cards as observers | `web/src/terminal/peek.ts` |
| Vibe menus answered from cards again | `web/src/data/actions.ts`, adapters |
| CLI: attach with its window, `VIEWER_STATE`, wrap off and status line | `crates/cli/src/{client.rs,session_size.rs}` |

Compatibility: an installed CLI stops on an unknown message type, so
`SESSION_SIZE` goes only to viewers that sent a size. Old clients' `RESIZE`
still sets the size at once. New clients facing an older daemon (a worker
behind the control) find no `SESSION_SIZE` before the screen and fall back
to `RESIZE`.

### Tests

Policy (9): alone takes the size; joining does not; two viewers alternating
after the settle time; a window drag resizes once; focus and blur; the active
viewer leaving; observers; old `RESIZE`; bounds. Through the manager (5,
`crates/daemon/tests/session_size.rs`): hand-over with audit records and the
PTY's size; leaving; observers may not type; old client; the snapshot redrawn
at the new size. Answers (3): keys from the marked row (both cursor-key
modes); the selector moved since the question; refusals. Protocol: sized
`ATTACH` validation. CLI (4): owner, wider, smaller, status width. Web: size
arithmetic (5), the hook against old and new daemons, peek as observer, Vibe
answers through `ANSWER_PROMPT`. Browser: two windows of different sizes,
typing in turn, rows identical; the bar moves; "Use this size".

`just check`: passed. Rust 1,462 passed, 0 failed, 22 ignored; web 592
passed (108 files).

`just test-e2e`: 21 passed (2.1 min). The first run failed one existing
layout test: the window that set the size shrank its font while waiting for
the daemon to confirm a narrower size. Fixed: that window now refits at once.

### Tried by hand (QA instance, web on 127.0.0.1:5283)

A phone-width tab opened a Session alone and took its size (long lines
wrapped at its edge). A second tab at 1440×900 joined: it showed the same
91×32 screen and "Sized for another device (91×32) · Use this size". One tap
moved the size to the wide tab (95×31 in the pane); the phone tab then
showed the wide screen unwrapped at a smaller font, with the bar offering the
size back.

### Not verified

- The CLI side by hand: covered by its unit tests and the build only.
- `ANSWER_PROMPT` against a real Vibe menu: covered by unit tests on the key
  arithmetic and refusals, not run against the program.
- Panning in a phone's full-screen terminal: touch drags scroll the
  scrollback there, so a Session wider than the phone at 9 px pans only with
  a trackpad or mouse.

## T4: `sentinal watch` picks up from anywhere (2026-11-22)

Design: `docs/WATCH_PICKUP_DESIGN.md`, approved by the owner on 2026-11-22
with the four recommended decisions. Tested on a working tree based on
commit `b27e643`.

### What was built

| Part | Where |
|---|---|
| Who can be picked up (live, FULL, reachable), order (waiting, running, idle; newest change first), matching, the picker's table and answers | `crates/cli/src/pickup.rs` (pure) |
| Bare `watch` picks up; `NAME\|ID`, `MACHINE:ID`, `--last`, `--new`, `--machine`, `--json`; never prompts without a terminal (exit 2); one Session attaches at once, none starts a shell | `crates/cli/src/watch_pickup.rs`, `args.rs`, `app.rs` |
| History on pick-up, after scrolling the window's own screen into its scrollback | `crates/cli/src/client.rs` (`pick_up`) |
| `sentinal login` / `logout`: the browser's owner sign-in from the terminal, kept 0600 in `owner-session`; other Machines from `/api/v1/control/snapshot`, attached through `/api/v1/machines/{id}/ws` | `crates/cli/src/login.rs` |

No control or wire change: the control's `/api/v1/auth/login` already issues
the owner sign-in a browser gets.

### Tests

Pick-up rules (9): order, `--last`, IDs here first then anywhere,
`MACHINE:ID`, title then agent or folder then a title or word start,
ambiguity, picker answers, running times, the table, what cannot be picked
up. Arguments: target, `--last`, `--machine`, `--new`, and the refused
combinations. Login (4 unit, 2 against a stand-in control with the real
contract): only encrypted origins get the password; the terminal route URL;
the file is 0600 and forgotten once expired; a wrong password keeps nothing;
the kept sign-in reads the list; a token the control no longer accepts reads
as signed out; logout tells the control; an unreachable control. Real
control: the token from `/api/v1/auth/login` reads the snapshot and logout
ends it.

Found while checking: a T3 test (`a_size_change_redraws_the_screen_at_the_new_size`)
attached before the reader had drawn the output, so it failed about one run
in five; the failure skipped the fake PTY's exit and the test runtime then
waited forever, which hung `just check` for an hour. Fixed by waiting until
the output is on the screen; 60 runs of the file passed afterwards.

### Tried by hand (QA instance, real terminals in the Terminal panel)

1. A Session ran a script that printed 120 build lines and asked "Deploy to
   staging? [y/N]", started with `--detach` (nobody attached).
2. `sentinal watch` without a terminal listed it first, WAITING, with IDs,
   said "Only this Machine. `sentinal login` …", and exited 2; `--json`
   printed the same list; `watch zzz` exited 1 with the list.
3. `sentinal watch deploy` in a terminal found nothing: the last rule only
   matched the start of the title. Changed to match any word, then it
   picked the Session up: all 120 lines in the terminal's scrollback, then
   the question.
4. The phone layout showed the question with y / n; tapping y reached the
   picked-up terminal at once ("you said: y").
5. Closing that window only detached. `sentinal watch --last` in a new one
   came back to the same Session, its history and the answer, with the
   window's own earlier output kept above it (the first try drew over the
   prompt line; fixed by scrolling the screen away first).

### Not verified

- `sentinal login` by hand and picking up a Session on another Machine: the
  QA setup has one Machine, and the password prompt needs a person at the
  terminal. Covered by the tests above; the routed terminal itself is the
  browser's, covered by `projection-acceptance` in the browser tests.
- The picker's numbered prompt by hand (needs typing at the terminal); its
  answers are unit-tested.
- With `claude` itself, as T4.4 asks: a script with a `[y/N]` question was
  used instead, so no model was run.

## T5: keepers, terminals that survive a daemon restart (2026-11-24)

Design: `docs/R1_KEEPER_DESIGN.md`, approved by the owner on 2026-11-24
with every recommended answer (A1, Q1–Q7). Tested on a working tree based on
commit `3118401`, macOS 27.0 (Apple silicon).

### What was built

| Part | Where |
|---|---|
| Frames, limits, handshake, HMAC proof, checkpoint pieces | `crates/daemon/src/keeper/protocol.rs` |
| The 1 MiB ring with absolute offsets and gaps | `keeper/ring.rs` |
| Socket directory per data directory, `0700`/`0600` checks, peer UID (`nix`) | `keeper/paths.rs` |
| `sentinald keeper`: PTY, shell, ring, one authenticated controller, backpressure while connected, overwrite while not, `HUP` then `KILL` after 5 s, 24 h orphan timeout, `Shutdown` from any process of the user | `keeper/process.rs` |
| Daemon side: start (own process group; own systemd scope under systemd), claim with every §7 check, present as a PTY, checkpoints | `keeper/client.rs`, `keeper/mod.rs` (`KeeperPty`, falls back to the daemon's own PTY) |
| Store: `session_keepers` (migration 016); `interrupt_orphaned_sessions` spares kept Sessions; `interrupt_session` with a reason | `crates/store/src/session_keepers.rs` |
| Log as independent zstd frames, `sessions/<id>.offset`, checkpoints every 5 s at clean points | `session_manager/kept.rs`, `tasks.rs` |
| Reclaim at startup: cut the log back, checkpoint + output to the screen, new output through log and watcher, running Command resumed, a redraw nudge after a gap | `session_manager/reclaim.rs`, `tracker.rs` (`resume`) |
| A keeper that dies ends only its Session (interrupted, with the reason) | `tasks.rs` |
| `Session.kept`; update warnings (CLI, worker, control) count only terminals no keeper holds | `protocol`, `entry.rs`, `cli/update_guard.rs` |
| `sentinald keeper stop-all`; uninstall and a rollback before 2.12.0 run it; launchd `AbandonProcessGroup` | `bin/`, `scripts/` |

### Found and fixed while trying it

A second daemon started on the same data directory while one was running
(by hand here; a service manager could do the same) reclaimed the running
daemon's keepers and only then stopped on the "another daemon is accepting
connections" check, leaving those terminals cut off from both. The same
ordering is why a mistaken start on 2026-11-22 marked a Session of the
installed daemon interrupted. The daemon now takes its local socket before
it touches any Session; a test starts a second daemon and checks the first
one's terminal still works.

### Tests

Keeper protocol and ring (13), socket paths and peer checks, framed log cut
at a closed frame, offset file, checkpoint restore of both screens and
history, store keeper rows and the orphan step (3), update warning.
Real processes (`crates/daemon/tests/keeper_restart.rs`, 6, each ending its
own keepers): SIGKILL mid-output and restart (same shell PID, output from
the outage on screen, typing reaches the shell, each line once in the log,
Stop ends the shell); a full-screen program after 2 MB of output, restored
from the checkpoint (the daemon logs `checkpoint=true`); a keeper killed
with SIGKILL ends only its terminal; a tampered record fails closed; a
keeper speaking protocol 99 gets `Shutdown` and its Session is interrupted;
a second daemon changes nothing. Ran 8 times in a row without a failure.

### Tried by hand (QA instance)

Two terminals, `top -s 1` and a counter printing a line a second. After the
QA daemon was killed with SIGKILL and started again, both were RUNNING, the
daemon logged both reclaimed from checkpoints, the web terminal showed `top`
live, and the counter's log held ticks 0–166 with no gap and no repeat across
two restarts.

### Differences from the design

- **No `stream_offset` columns.** The daemon resumes from the last closed log
  frame (at most 1 s or 64 KiB back). Output in that window is logged once
  (the log is cut back first), but Events and Commands the watcher saw in it
  can be recorded twice after a restart. The design's per-row offsets would
  remove that; not done.
- **Reasons as text.** An interrupted Session's reason is the text of its
  warning Event (§8 wording), not a new structured field.
- **§7 check 4** (the keeper runs the installed `sentinald`) is not checked;
  the PIDs with start times and the HMAC proof are.
- **No note in the web app** for a terminal no keeper holds; `Session.kept`
  carries it, the UI does not show it yet.

### Not verified

- Linux: the systemd scope, peer credentials (`SO_PEERCRED`) and keeper
  memory were not run on Linux; Q5's measurements are still to be taken on
  a Linux worker before release.
- `sentinal update`, a worker update from the dashboard, and the control's
  self-update with live terminals: covered by the counting tests, not run.
- Uninstall and rollback ending keepers: script syntax and installer tests
  only; `stop-all` itself is the code the restart tests use to clean up.
