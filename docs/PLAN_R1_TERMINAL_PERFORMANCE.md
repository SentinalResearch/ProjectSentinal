# Plan: keepers (R1), terminal fidelity, reliability, performance

Status: proposal for owner review (2026-10-23). Nothing here is approved.
R1 needs an approved architecture before PTY ownership moves
(`docs/ROADMAP.md`); section 1 is that proposal.

The four problems reported from real use:

1. Updating or restarting `sentinald` ends every live terminal.
2. A Sentinal terminal does not feel like the user's own terminal.
3. Failures are handled poorly (silent drops, unclear states).
4. The dashboard is sometimes slow to load, sometimes shows a worker
   offline when it is not, and is sometimes plainly slow.

Facts found in the code while writing this are marked **Found**.

---

## 1. Keepers: terminals that survive a daemon restart (R1)

### Today

`sentinald` owns each PTY master (`portable-pty`), the shell is its child,
and the vt100 parser and log writer run in the daemon. Any daemon exit
(update, crash, `systemctl restart`, launchd reload) closes the PTY masters,
the shells get SIGHUP, and `interrupt_orphaned_sessions` marks them
INTERRUPTED on the next start.

The service managers make this worse: systemd's default
`KillMode=control-group` kills every process in the service's cgroup when the
service stops, and launchd kills the job's process group. So "just don't
close the fd" is not enough; whatever holds the PTY must live outside the
service's kill scope.

### Options

| | A. Keeper per Session | B. One keeper server | C. Re-exec in place | D. systemd FD store |
|---|---|---|---|---|
| Model | dtach/abduco: one small process holds one PTY | tmux server: one process holds all PTYs | daemon `execve`s the new binary, PTY fds inherited | systemd keeps the fds while the service restarts |
| Survives update | yes | yes, unless the keeper itself updates | yes | yes |
| Survives daemon crash | yes | yes | **no** | yes |
| Survives keeper crash | only that Session dies | **all** Sessions die | n/a | n/a |
| macOS | yes | yes | yes | **no** (Linux only) |
| Extra processes | 1 per live Session | 1 total | 0 | 0 |
| Memory (estimate, to be measured) | ~1–2 MB private RSS each + its output ring | ~3–5 MB + rings | 0 | 0 |
| Complexity | medium: spawn, reclaim, protocol | medium-high: multiplexing, its own upgrade problem | low-medium: state handoff across exec | low, but Linux-only and still loses crash state |
| Updating the holder itself | old keepers keep running old code until their Session ends; a versioned protocol keeps them reclaimable | restarting the server kills everything | n/a | n/a |

Rejected outright: using tmux/screen as the backend (new runtime dependency,
not installed everywhere, and its own state model fights ours).

### Recommendation: A, keeper per Session, built into the existing binary

- **Same binary, new mode.** `sentinald keeper …` — nothing new to ship,
  sign, or update. The keeper uses a plain `poll` loop, no tokio runtime, so
  its private memory stays small; the binary's code pages are shared with
  the daemon by the OS.
- **What the keeper owns:** the PTY master, the shell (its child), and a
  bounded ring of recent raw output (default 1 MiB). Nothing else.
- **What stays in the daemon:** the vt100 screen, the log file, the OSC/
  command tracker, events, metrics, everything network-facing. The keeper
  never parses or interprets output, so a keeper bug cannot corrupt state
  and the keeper rarely needs to change.
- **Escaping the kill scope:** the keeper calls `setsid()`. On Linux the
  daemon starts it with `systemd-run --user --scope --unit=sentinal-keeper-<id>`
  when running under systemd (own cgroup, still visible in `systemctl --user
  list-units`), falling back to a plain double-fork elsewhere. On macOS the
  plist gains `AbandonProcessGroup = true` so launchd leaves it alone.
- **Reclaim after restart:** each keeper listens on a Unix socket in
  `$XDG_RUNTIME_DIR/sentinal/keepers/` (macOS: the daemon's private data dir),
  directory `0700`. On connect the daemon checks the peer UID
  (`SO_PEERCRED`/`getpeereid`) and a per-keeper secret stored in the
  daemon's SQLite row. The keeper sends its ring; the daemon replays it into
  a fresh vt100 parser at the current size, then resumes streaming.
- **Fail closed:** a socket with the wrong owner, a bad secret, an unknown
  protocol version, or a keeper whose shell PID no longer matches → the
  Session is marked INTERRUPTED exactly as today. A keeper with no Session
  row is told to hang up.
- **Orphan cleanup:** a keeper whose daemon has not reconnected for
  `keeper_orphan_timeout` (default 24 h) hangs up its shell and exits, so a
  removed Sentinal never leaves processes behind. `sentinal uninstall` ends
  all keepers.

### What survives, what does not

| Event | Result |
|---|---|
| `sentinal update`, dashboard Update, `systemctl --user restart` | Session continues; screen rebuilt from the ring. Output during the gap (usually <2 s) is in the ring, so nothing is lost unless the gap produces more than 1 MiB. |
| Daemon crash | Same as above. |
| Keeper crash | That one Session INTERRUPTED (as today); others unaffected. |
| Reboot, logout without linger | Shells die; INTERRUPTED; "Resume terminal" as today. |
| Command tracker mid-command | The running command's start is in SQLite; its end is detected normally. Output that was only in the gap is replayed to the tracker from the ring. |

### Performance and memory

- **Data path:** PTY → keeper → Unix socket → daemon adds one copy and one
  context switch per chunk (tens of µs). Network latency to the browser is
  milliseconds, so this is not measurable to the user.
- **Idle cost:** a keeper blocked in `poll` uses no CPU.
- **Memory per live Session (estimates, measure in the spike):** keeper
  ~1–2 MB private + 1 MiB ring. 20 live terminals ≈ 40–60 MB total.
- This is cheaper than a waste we already have (**Found**, section 4.3):
  the daemon's vt100 parser keeps a 10 000-line history at 32 bytes per cell
  (~38 MB per Session at 120 columns once full) that attach never sends.
  Fixing that pays for the keepers several times over.

### Delivery steps

1. Spike (1–2 days): keeper mode + reclaim on macOS and Linux under the real
   service managers; measure RSS and latency. Go/no-go.
2. Keeper protocol (versioned, length-prefixed frames: data, resize, signal,
   exit, ring-replay) with unit tests in `crates/daemon`.
3. `PtyBackend::Keeper` behind the existing `PtyBackend` trait, so `FakePty`
   tests keep working; `RealPty` stays as a fallback when keepers cannot be
   started.
4. Reclaim at startup replaces `interrupt_orphaned_sessions` for keeper-backed
   Sessions; fail-closed tests for every mismatch above.
5. Service file changes (systemd scope, launchd `AbandonProcessGroup`) in the
   installer and `sentinal setup`.
6. Acceptance: update a worker while `top`, `vim`, and a long build run in
   three Sessions; all three continue, screens intact, logs complete.

Until it lands: the Update button warns "This will end N live terminals on
<Machine>".

---

## 2. Terminal fidelity: "it feels like my own terminal"

Already fixed in 0.2.4: the shell gets the user's environment (PATH etc.)
instead of the service's.

| # | Gap | Found / suspected | Fix |
|---|---|---|---|
| 2.1 | Opening a running Session shows only the current screen; scrolling up shows nothing earlier | **Found**: attach sends `screen().state_formatted()` (visible screen only) | Send the last N scrollback rows (formatted) before the screen; or replay the log tail for the first attach |
| 2.2 | Heavy output can corrupt the browser screen | **Found**: when a viewer lags the broadcast channel, `api.rs` logs "attached terminal client lagged" and **drops** the bytes; escape sequences are lost | On lag, send a fresh snapshot (reset + screen state) instead of continuing with a gap |
| 2.3 | Non-UTF-8 shells on Linux (`?` for Unicode, Python encoding errors) | **Found**: the daemon never sets `LANG`; SSH gets it from PAM (`/etc/default/locale`), `bash -l` does not | If the login environment has no `LANG`/`LC_ALL`, read `/etc/default/locale`, else default `C.UTF-8` |
| 2.4 | bash startup differs from SSH | Dashboard bash runs non-login with `--rcfile`; SSH runs a login shell (`~/.bash_profile`/`~/.profile` then usually `~/.bashrc`). Aliases/functions defined in `.bash_profile` are missing | Run bash as a login shell with our hook injected via `PROMPT_COMMAND`/`BASH_ENV`, or source the user's login files from the wrapper in SSH order |
| 2.5 | Emoji/CJK misaligned | No `@xterm/addon-unicode11` | Add it (same project as xterm; small) |
| 2.6 | URLs not clickable | No `@xterm/addon-web-links` | Add it; open in new tab with `noopener` |
| 2.7 | Copy from vim/tmux (OSC 52) does nothing | No clipboard handling | Add `@xterm/addon-clipboard` with a confirm-once permission |
| 2.8 | Prompt glyphs (powerline, starship, p10k) show boxes | Only JetBrains Mono | Font setting; offer a Nerd Font variant |
| 2.9 | Option/Alt keys on macOS, browser-reserved shortcuts (Ctrl-W, Ctrl-T, Ctrl-N) | `macOptionIsMeta` not set; browser takes the shortcuts | Setting for Option-as-Meta; in the installed PWA, capture reserved keys; document what the browser cannot release |
| 2.10 | Window title, bell | Ignored | Show OSC 0/2 title on the tab and Session header; visual bell |
| 2.11 | Two viewers with different sizes fight | Last resize wins (known) | Size to the smallest active viewer, or "this view controls size" toggle |
| 2.12 | Browser scrollback 4000 lines | `scrollback: 4000` in `TerminalPane` | Make it a setting; default 10 000 |

Acceptance for this track: a scripted "terminal torture" run (vim, htop,
tmux inside, `cat` of a 50 MB file, emoji/CJK file, starship prompt,
OSC 52 copy, resize storm) in the e2e suite plus a real-hardware checklist.

---

## 3. Reliability: handle failures better

| # | Problem | Fix |
|---|---|---|
| 3.1 | "Offline" when a worker is only slow (see 4.1) | Distinguish *delayed* from *offline* in the UI |
| 3.2 | Silent output loss on lag (2.2) | Resnapshot |
| 3.3 | Update ends terminals without warning | Warning now, keepers later |
| 3.4 | Failures without a reason (the New session start was silent before 6d9fbe4) | Audit every dashboard action for an error path that shows the daemon's message; add a test per action |
| 3.5 | Reconnect after laptop sleep / network change | Browser: reconnect immediately on `online`/`visibilitychange` instead of waiting for the backoff timer; worker link: same on network change |
| 3.6 | Stale installs (pilot control, empty `control_url`) | Done in 0.2.4; add a `sentinal doctor` that checks version, control URL, link, service, rc blocks, and prints the fix |

---

## 4. Performance

Measure first: add timings before changing behaviour. Without numbers we will
fix the wrong thing.

### 4.1 False "offline" (**Found**, likely the main cause)

- Worker metrics are sampled every 2 s and uploaded as projection deltas
  every 1 s.
- The control calls a worker's projection LIVE only if the last upload was
  ≤ 10 s ago (`PROJECTION_LIVE_MS`); otherwise STALE.
- The dashboard maps STALE to **OFFLINE** (`projection-view.ts`).
- So any 10 s hiccup (a DERP-relayed Tailscale path, a busy VM, one slow
  HTTP request — the upload timeout is 15 s) shows the worker as offline,
  even though heartbeats (every 30 s, offline after 90 s) say it is up.

Fix: show STALE as "online · last update Ns ago" (amber), keep OFFLINE for
the heartbeat verdict, and allow routed actions on a STALE Machine (they go
over the live proxy, not the projection).

### 4.2 Slow first load

Suspects, in the order to measure:
1. Tailscale path: `tailscale ping <worker>` from the control. A relayed
   (DERP) path adds 50–200 ms per round trip, and browser → control → worker
   doubles it.
2. Projection snapshot size on first load (events and metrics history for
   every Machine).
3. Per-worker WebSocket setup through the proxy (`prove_identity` round trip
   per worker, 5 s connect timeout when one is unreachable) — check that one
   dead worker never blocks the page.
4. Split View opens one WebSocket per pane (noted in
   `useRealTerminalSession.ts`).

Instrumentation: `Server-Timing` headers on the control's API, a
`performance.mark` timeline in the PWA (behind a debug setting), and a
`sentinal doctor --perf` that runs `tailscale ping` and times one proxied
round trip per worker.

### 4.3 Daemon memory (**Found**)

The vt100 parser is created with `scrollback_lines` = 10 000. At 32 bytes per
cell that is ~38 MB per Session at 120 columns (~64 MB at 200) once the
history fills, and attach never sends it. Either use that history for 2.1
(with a much smaller default, e.g. 2 000 rows) or set it to 0 and serve
history from the log. Measured idle daemon today: 19 MB RSS.

### 4.4 Steady-state load

Metrics every 2 s → an upload every second per worker → a SQLite write on the
control per worker per second, even when nobody is looking. Sample every 2 s
only while a dashboard is open (the control already knows), every 15–30 s
otherwise.

---

## 5. Proposed order

| Phase | Contents | Why first |
|---|---|---|
| P0 (days) | 4.1 delayed vs offline; 2.2 resnapshot on lag; 2.3 locale; update warning; timing instrumentation | Small, removes the most visible pain, and gives numbers for P2 |
| P1 (1–2 weeks) | R1 keeper spike → implementation (section 1) | The biggest trust problem; needs your approval |
| P2 | 2.1 scrollback on attach + 4.3 memory; 2.4 bash login semantics; 2.5–2.8 xterm addons and font | Fidelity |
| P3 | 4.2 load fixes from measurements; 4.4 adaptive metrics; 3.5 reconnect; 3.6 doctor; 2.9–2.12 | Polish |

## 6. Decisions needed from the owner

1. Approve keeper option A (per Session, inside `sentinald`), or choose B/C.
2. Ring size per keeper (1 MiB proposed) and orphan timeout (24 h proposed).
3. On Linux, OK to run keepers in their own `systemd-run --user --scope`?
4. Should the dashboard ever be allowed to act on a STALE Machine, or only
   show it?
5. Phase order above, or keepers before P0?
