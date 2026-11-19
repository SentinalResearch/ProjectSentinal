# Tasks: everything left to do, for any coding agent

Status: **2026-11-08.** This is the single list of open work. Each task can be
handed to a coding agent on its own. Start with "Before any task", then pick one
task by its ID.

Where this list and an older plan disagree, this list wins. The detailed reasons
live in `docs/PLAN_DAILY_USE_AND_WATCHER.md` (Tracks A–D) and `docs/PLAN_V2_AI.md`.

## Before any task

Read these first, in order:

1. `AGENTS.md`: the rules. Tests first, about 400 lines per file at most,
   `just check` clean, no `unwrap()` in library code, protocol changes update the
   fixtures on purpose.
2. `docs/ARCHITECTURE.md`: crates `protocol`, `store`, `watcher`, `daemon`
   (`sentinald`), `cli` (`sentinal`), and `web/` (React 19, Vite, xterm).
3. `docs/TESTING.md`: how to test, including the isolated dev environment.
4. The section of this file for your task.

### Hard rules

- **Never touch production.** The owner runs Sentinal every day on five
  Machines. Test only with `just dev` (web on port 5273, daemon on 7520, data in
  `~/.local/share/sentinal-dev`, `isolated = true`). A daemon you start must never
  use `~/.config/sentinal`, the real `link.json`, or the installed data
  directory. If you are unsure whether something reaches production, stop and ask.
- **No secrets in commits:** no private keys, auth keys, passwords, Tailnet
  names, or public or Tailscale IP addresses. Scan the diff before committing.
  Never commit `install.sh`, `screenshot_issues/`, `__pycache__`, or
  `docs/guide/sentinal-guide-overleaf.zip`.
- **Commit, push, tag, and release only when the owner asks.** Work on the `dev`
  branch. `main` gets work only through `just promote`, and releases only through
  `just ship X.Y.Z` run by the owner. The next release is **v2.11.0**; do not
  release it.
- **Do not run commands on the owner's VMs.** Write the command; the owner runs it.
- The product is spelled **Sentinal** (binaries `sentinal`, `sentinald`,
  variables `SENTINAL_*`). Use the spec's words: Machine, Session, Command,
  Event, Recipe, Process, Metric.

### Build and check

```sh
export PATH="$PATH:$HOME/.cargo/bin:/opt/homebrew/bin"
export CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4
just check        # fmt, clippy -D warnings, Rust tests, TS types, web tests
just test-e2e     # browser suite (uses a fake PTY: real programs do not run)
just dev          # isolated dev environment to try changes by hand
just dev-cli ls   # CLI against the dev daemon
```

macOS notes: there is no `timeout` (use `perl -e 'alarm 60; exec @ARGV' cmd`);
delete `web/tsconfig.tsbuildinfo` after checks; zsh does not word-split
variables. A test that starts a fake Session must exit it (`fake.exit`), or the
tokio runtime hangs on shutdown.

### Definition of done for every task

- A test that fails without the change, then the change.
- `just check` passes, and `just test-e2e` passes if the web changed. Report the
  real output, not a summary of what should happen.
- Checkboxes updated in `docs/PLAN_DAILY_USE_AND_WATCHER.md` and this file.
- A protocol change updates `crates/protocol/tests/fixtures/` on purpose, says so
  in the commit message, and is noted under "Release notes" below.
- Tried by hand in `just dev` when it is visible to a user, with what you saw.

## Prompt template for another model

Copy this, filling in the task ID:

```text
You are working in the Sentinal repository on the `dev` branch.
Read AGENTS.md, docs/ARCHITECTURE.md, docs/TESTING.md, then docs/TASKS.md.
Do task <ID> from docs/TASKS.md, and only that task.
Follow "Before any task" and "Definition of done" in docs/TASKS.md exactly.
Never touch production: test only with `just dev`.
Do not commit, push, tag or release. When finished, report: what changed (files),
tests added, the real output of `just check` (and `just test-e2e` if the web
changed), what you tried by hand, and anything you could not verify.
If the task changes the protocol and something is unclear, stop and ask.
```

## State right now

- `main` = `origin/main` = tag **v2.10.0** (production).
- `dev` has 11 local commits (not pushed), including the screen
  question detector and dashboard prompt fixes (task **P0**, now committed).
- **2026-11-08 agent pass:** B2 (::sentinal progress/metric API), B1
  (METRIC_REPORTED + server detection), P1 (Vibe arrows+Enter), P2 (marked
  menus), P4 (Claude "Running N shell command…"), P5 (agent Session names),
  P6 (agents filter), A2 (every route has a place), A4 (prompt hide/restore/
  delete) — all done with tests.

---

## X: bugs from the 2026-11-12 review

A review read about a third of the code. Every fix below has a test that
fails without it. Items marked *(v2.10.0)* are also in production.

### Fixed (committed in `64117cf`)

- [x] X1 Agent Events never left the engine: `supports_evidence` had no arm for
      `AgentActivity` / `AgentTurnFinished`, and the notification rules mapped
      `AgentTurnFinished` to "disabled". No "agent finished" notification was
      ever sent *(v2.10.0)* — `watcher/tests/agent_events.rs`
- [x] X2 An emoji before `loss` in a training line panicked the watcher (byte
      slice in `training.rs`) *(v2.10.0)* — `watcher/tests/unicode_lines.rs`
- [x] X3 Bash Sessions recorded `$BASH_COMMAND`: aliases expanded and only the
      first command of a pipeline or list (`ls --color=auto --color=auto` in
      completion) *(v2.10.0)* — `scripts/tests/bash-preexec-test.sh`
- [x] X4 Any line-feed byte detached an attached terminal and dropped the rest
      of the input; now only a lone Ctrl+J keypress outside a bracketed paste
      *(v2.10.0)* — `cli/tests/cli.rs`
- [x] X5 A worker with more than about 60 Sessions could never resync a fresh
      control (16-chunk snapshot cap); now 1,024 chunks, bounded at 64 MiB
      staged *(v2.10.0)* — `store/tests/projection.rs`
- [x] X6 Outbox compaction rewrote a change already being uploaded, and its
      acknowledgement deleted the rewrite (dashboard RUNNING while WAITING)
      *(v2.10.0)* — `store/tests/projection.rs`
- [x] X7 A second question within 5 minutes never notified (worker and
      control); finished agent turns were also folded on the worker. One rule
      per category now: finishes always, questions after 30 s, others 5 min
      *(v2.10.0)* — `notifications/tests.rs`, `store/tests/native_push.rs`
- [x] X8 Killing a Session left its Command RUNNING forever and sent viewers no
      update *(v2.10.0)* — `daemon/src/tracker.rs` tests
- [x] X9 Server ports were read from the address or a timestamp (`127.0.0.1:8080`
      gave 127) — `watcher/tests/generic.rs`
- [x] X10 Vibe answers pressed Down (N−1) times assuming the marker is on option
      1. Interim: Vibe questions are answered in the terminal (no one-tap
      answer). Real fix needs the watcher to report the marked option: a
      protocol change, **owner to decide**
- [x] X11 `sentinal detach <id>` hung and could never work (attachments are per
      connection); removed, help points to Ctrl+J
- [x] X12 `::sentinal` lines were not bounded: metrics now one per name per 5 s
      and at most 32 names, `unit` ≤ 32 and names limited to `[A-Za-z0-9_.-]`,
      any malformed field rejects the line
- [x] X13 `MetricReported` sent a program-printed name to the control; only an
      allowlisted name (`loss`, `accuracy`, `lr`, …) goes, else `metric`

### Still open

- [x] X14 Health stays WARNING after a question is answered, for the rest of
      the Command (`watcher/src/health.rs:138`); agent Sessions are one Command
      (fixed as T0.6)
- [ ] X15 Rollback from 2.11: stored rows are decoded strictly, so
      `METRIC_REPORTED` Events and `VIBE`/`ANTIGRAVITY` Sessions make whole
      queries fail on 2.10 (`store/src/records.rs:156`, `lib.rs:354`)
- [ ] X16 Nothing prunes `events`, `commands`, `metrics_minute` or the
      control's notification tables; every warning line is an Event
- [ ] X17 The projection recorder decodes every stored Session twice a second
      and re-records all of them on each restart (`projection_runtime.rs:32`)
- [x] X18 A second identical question within 1 s of the first ending was not
      reported. The daemon now tells the watcher that input arrived (never the
      bytes); a question that leaves the screen after input ends at once, so
      the same question again is new — `watcher/tests/screen_prompt.rs`
- [x] X19 Marked menus allowed 8 options; the protocol and web cap at 6. A
      marked menu now offers its first 6 rows, like a numbered menu —
      `watcher/tests/screen_prompt.rs`
- [x] X20 The control dashboard named every agent Session just "Claude Code".
      Now "Claude Code · Session N" (`data/format.test.ts`). Showing the folder
      there needs the projection to carry it: a protocol and privacy change,
      **owner to decide**
- [x] X21 The prompt's delete dialog had no Escape handling or focus trap. It
      is now a `ConfirmDialog`, which traps Tab, returns focus to its opener,
      and for delete starts on Cancel — `shell/GlassShell.test.tsx`
- [ ] X22 Browser logins live only in memory and end on every daemon restart
- [x] X23 A panic in the watcher still ends the PTY reader; catch it there so
      one bad line can never stop a Session being read (fixed as T0.3)
- [x] X24 From screenshots: on the phone Session page the Open terminal button
      covers Health · Evidence; the prompt bar covers the last Machine in the
      Start a Session sheet; an offline Machine's Session still shows IDLE.
      The first two were already fixed (the button is in the page and scrolls
      away; the sheet renders on `<body>` above the prompt bar), checked at
      375 px. A live Session on a Machine that is not online now shows
      DISCONNECTED everywhere (`withMachineReach` in `LiveDataContext`); its
      real state returns with the Machine
- [ ] X25 The unread two thirds: most of `daemon/src/control`, link, push,
      package service, config and PTY code, most of the CLI and `web/src`,
      `scripts/`, `intelligence/`, `eval/`

### Q: hands-on run in a dev instance, 2026-11-19

Used like an owner would, in an isolated dev instance: a training script
with `::sentinal` lines, a noisy build, a `[y/N]` script asked twice, a
Python web server, and real Claude Code 2.1.328 (trust menu, two identical
approvals). Each fix has a test that fails without it.

- [x] Q1 Training progress read "18 %" with "% 17.5/100" (island: "18 % %
      17.5/100"): the dashboard's Health percentage replaced the program's
      `epoch 7/40`. Projected progress Events now carry the counts, a bare
      percentage never replaces them, and one shows as "17.5 %" —
      `format.test.ts`, `projection-view.test.ts`
- [x] Q2 The whole app went blank ("Maximum update depth exceeded") when a
      question arrived: the island's ticker measured its room from the
      window while the island moved to centre itself by its width. Now
      measured from the island's own edge (`LiveIsland.tsx`)
- [x] Q3 Health said "7 errors" for a build that printed 12: it counted the
      Events kept from a burst. It now counts every one — `rate_limit.rs`
- [x] Q4 Every child process (each `sleep` in a script, each tool an agent
      runs) added "Process started" and "Process exited" Events. Terminals
      that track Commands no longer do; adopted processes still do —
      `processes.rs`
- [x] Q5 Claude Code 2.1.328 showed "0 tool calls" after running commands:
      it now draws `● Creating empty file one.txt` over `⎿ $ touch one.txt`.
      The `⎿ $` line is read — `agent_activity.rs`. Not done: the collapsed
      `Ran 2 shell commands` summary, whose same text repeats across turns,
      so very quick calls can still be missed
- [x] Q6 Claude Code's trust menu was asked as "Security guide" (a link
      above it). A hinted menu with no question right above it now finds the
      wrapped sentence ending in `?` — `screen_prompt.rs`
- [x] Q7 Answer buttons on menus without numbers (Claude Code's trust and
      billing menus) send a digit, which those menus ignore, so the button
      silently did nothing. The card now says so after 3 s and points to the
      terminal (`PromptChoices.test.tsx`). The real fix is T3.4 (arrow keys
      from the daemon) — **owner to decide** whether to bring it forward
- [x] Q8 `run python3 train.py` in the island did nothing on Enter (`run` is
      for Recipes). With no matching Recipe it now offers "new session" and
      says why — `promptGrammar.test.ts`
- [x] Q9 Python's `Serving HTTP on … port 8765` was not a server; nor are
      Flask's `* Running on http://…` or Next.js's `started server on …`.
      Now they are — `generic.rs`
- [x] Q10 Events from the dashboard read "Command started / Command
      started", "Warning / Warning detected": the detail only repeated the
      label. The list leaves it out — `EventsList.test.tsx`
- [ ] Q11 Confirmed X22 by hand: restarting the daemon signs every browser
      out, while the sign-in page says browsers "stay signed in until you
      sign out or the session expires"
- [ ] Q12 A plain shell Session stays "Session 4" while it runs
      `python3 serve.py`; the running Command could name it
- [ ] Q13 The phone Session page shows LOSS and GPU cells for an agent
      Session that has neither

### AC: agent Sessions as a chat · *built 2026-11-19, desktop*

Design: `docs/AGENT_CHAT_DESIGN.md` (approved, recommended decisions). Tried
with real Claude Code 2.1.328 in a dev instance: link, chat, live updates,
message box, and progress in the agent chip.

- [x] Owner answers "Decisions for the owner": as recommended
- [x] AC1 Link through Claude Code's `SessionStart` hook, and the reader;
      progress (task, step N of M, activity, working/done) as an agent report.
      Tests: `watcher/tests/agent_transcript.rs`, CLI `agent_chat_command`,
      protocol fixtures. Not done: `sentinal status` does not print the chat
- [x] AC2 Chat view on the desktop Session page (`AgentChatView`,
      `data/agentChat.test.ts`). Not done: the phone page, which needs an
      attach for live entries
- [x] AC3 Message box: text, then Enter as its own keypress
- [ ] AC4 Tool counts and turns from the file on the agent badge (the chat
      and its turn lines already use them; the badge still reads the screen).
      A linked Session should also count as Claude Code: one started as
      `cd … && claude` shows "agent says"
- [ ] Q14 After `new session` and Enter in the island, the places menu opens
      on the new Session page (seen three times in the run)
- [ ] AC5 Next agent, after capturing a real file from it

- **2026-11-12 audit and terminal design:** a read of the watcher, protocol,
  store, and the daemon's Session code found the bugs listed under **T0**, and
  the causes of cluttered and overlapping terminals. The owner decided the
  design in `prompts/13-terminal-upgrade-package.md`. Section **T** below is
  now the main work; it replaces Track **C** and the answering parts of
  **P1** and **P2**.

---

## H: terminal history and resume · *built 2026-11-19, not committed*

Design: canvas page "Terminal history & resume", boards H1–H3, approved by
the owner on 2026-11-19 (Resumed tags in chevron style).

- [x] H1 Reattaching brings the scrollback back, not only the visible screen:
      `ATTACH.history` (optional; the web sets it, the CLI does not). The daemon
      sends its vt100 scrollback rows, then pushes the viewer's lines into its
      own scrollback, then the usual screen (`session_manager/screen_history.rs`).
      Covers T2.4's "history in the attach snapshot" on today's screen engine
- [x] H2 One terminal per Session: earlier parts' saved output, oldest first,
      then the live part (`terminal/priorParts.ts`, `usePriorParts.ts`). The
      1–N part tabs and the stacked 220 px terminals are gone
- [x] H3 A Resumed tag before each part: when, how the part before ended
      (green exit 0, amber interrupted, red failed), part N / M. An xterm
      decoration over a blank line: never written into output, logs or copies
- [x] H4 History list on the Session page jumps to each resume; "Back to the
      latest" pill while scrolled up; the phone terminal shows the same history
- [x] H5 Hide and Delete know each Machine's release: 2.11 any finished
      Session, 2.10 only failed, older none. Unsupported Machines are never
      asked; Done lists say which Machine needs 2.11 and show "Hide · needs
      2.11"; Auto-clear skips them; raw daemon errors are reworded
      (`data/removal.ts`). Development builds (0.0.x) count as current
- Known limits: a viewer that lags gets a full reset (RIS) and loses the
  earlier parts until reload; at most 32 parts are drawn (older ones are
  named, their output kept); the live screen starts below the earlier parts,
  so a fresh shell shows an empty screen with history one scroll up
- Not done: the session ↔ dashboard disagreement seen on 2026-11-19 (a
  worker's Session 4 Interrupted while the control showed it running) is not
  investigated yet

## T: terminal upgrade package · *main work, target v2.12.0*

Goal: start work on one Machine and pick it up from any terminal, phone, or
browser with `sentinal watch`, on one correct screen. No tmux bridge. Full
detail, owner decisions, and the wire proposal are in
`prompts/13-terminal-upgrade-package.md`; hand that file to the coding agent
after `prompts/00-master.md`. Stages are done in order, one change set each.

Owner decisions (2026-11-12): one size per Session set by the latest active
viewer; `libghostty-vt` as the daemon's screen engine if the T2 build gate
passes; keepers in scope; `adopt` stays read-only.

### T0. Watcher and daemon fixes

Found by reading; the first five were also reproduced by running the code.
Several are the same bugs as X1–X13 above and were fixed there (`64117cf`):
T0.1 = X1, T0.2 = X2, T0.7 = X12, T0.9 = X7, T0.10 = X8, T0.13 = X5 + X6.
T0.4's port half is X9 (one Event per line is still open); T0.8's 8-option
half is X19, its other halves are X18 and X10. Still open: T0.3 (X23),
T0.5, T0.6 (X14), T0.11, T0.12 (X15).

- [x] T0.1 Agent Events are dropped: `supports_evidence` has no case for
      `AgentActivity` / `AgentTurnFinished` (`crates/protocol/src/evidence.rs`),
      so no agent activity and no "agent finished" notification
- [x] T0.2 Panic on a training line with a multi-byte character before
      `loss` (`crates/watcher/src/training.rs:282`)
- [x] T0.3 A watcher panic stops the PTY reader for that Session
      (`crates/daemon/src/session_manager/tasks.rs`). 2026-11-19: the watcher
      runs under `catch_unwind`; a panic logs once, starts the engine over
      with the same settings (`EventEngine::restarted`) and drops that one
      observation — `tasks.rs` tests, `watcher/tests/restart.rs`
- [x] T0.4 Wrong server port (`127.0.0.1:8080` gives 127), and one Event per
      matching line (`crates/watcher/src/generic.rs`). 2026-11-19: a server is
      reported once per port per Command — `watcher/tests/generic.rs`
- [x] T0.5 Tool calls counted again after each turn; a turn "finishes"
      during an approval dialog (`agent_cli.rs`, `agent_activity.rs`).
      Already fixed with P4/X18: screen-read tool lines count once while
      visible across turns, and an on-screen question holds the turn open —
      `watcher/tests/agent_events.rs`
- [x] T0.6 Health stays WARNING after a question is answered, and FAILED
      after one error line (`crates/watcher/src/health.rs`). 2026-11-19: an
      answered question clears its warning and evidence; an error line is a
      counted WARNING; only a failed exit is FAILED (three tests updated on
      purpose to the new rule) — `watcher/tests/health.rs`
- [x] T0.7 `::sentinal` lines: metrics not rate limited, `unit` unbounded,
      malformed fields half accepted (B2 said otherwise)
- [x] T0.8 Same question asked again within one second is not reported;
      marked menus allow 8 options, the protocol 6; marked row not reported.
      The first two are fixed (X18, X19). 2026-11-19, approved by the owner:
      `WAITING_FOR_INPUT.marked` (optional, 0-based, only within the offered
      options) is the row the selector was on when the question was asked.
      Moving the selector is not a new question. The web still sends no
      arrows from it (the owner may have moved it since); T3.4 computes the
      keys in the daemon from the current screen — `watcher/tests/screen_prompt.rs`
- [x] T0.9 A second question in the same Session within 5 minutes sends no
      notification (worker and control both deduplicate). Likely the cause of
      the A3 item "notifications for questions sometimes never arrived"
- [x] T0.10 `kill` leaves the Command running forever in history and tells
      no viewer
- [x] T0.11 No rate limit on warning and error Events. 2026-11-19: per
      Session, the first 5 of a 10 s burst are kept, then one "N more
      warnings in 10 s" Event; warnings and errors separately
      (`watcher/src/bursts.rs`) — `watcher/tests/rate_limit.rs`
- [x] T0.12 One unreadable stored row fails the whole query (matters after a
      rollback from a newer release). 2026-11-19: Session and Event lists and
      search skip and log rows this build cannot decode
      (`store/tests/unreadable_rows.rs`). This also covers X15 going forward;
      2.10 itself still decodes strictly
- [x] T0.13 Projection: an update can be lost when it is compacted into a
      row already being uploaded; a worker with more than about 2,000
      records can never send a snapshot to a new control

### T1. Display fixes without a protocol change

- [ ] T1.1 Unicode 11 widths in the web terminal
- [ ] T1.2 Debounced fit
- [ ] T1.3 CLI handles window resize and restores the local terminal on exit
- [ ] T1.4 CLI resizes before the snapshot
- [ ] T1.5 A viewer never starts inside an escape sequence
- [ ] T1.6 Detach key: `Ctrl-\ d` only; `Ctrl+J` goes to the program. The
      `sentinal detach` subcommand on `dev` hangs and cannot work as written:
      owner decides remove or redefine

### T2. Authoritative screen on `libghostty-vt`

- [ ] Build gate on all four release targets, with numbers; stop and report
- [ ] T2.1–T2.4 one `screen` module, engine thread per Session, every reader
      moved to it, history in the attach snapshot

### T3. One size per Session · *protocol change, proposal first*

- [ ] T3.1 `ATTACH` fields, `VIEWER_STATE`, `SESSION_SIZE`, fixtures
- [ ] T3.2 Size policy in the daemon (latest active viewer)
- [ ] T3.3 Web renders at the Session size; "Use this size" bar; peek cards
      observe only
- [ ] T3.4 Answer keys for marked menus computed in the daemon from the
      current screen; the Vibe-only arrow logic in `web/src/data/actions.ts`
      (which assumes option 1 is marked) is removed
- [ ] T3.5 CLI follows the Session size
- [ ] T3.6 Two-viewer browser test

### T4. `sentinal watch` picks up from anywhere · *design first*

- [ ] T4.1 `sentinal watch` with no argument lists and attaches; `NAME|ID`,
      `--last`, `--new`, `--machine`
- [ ] T4.2 Sessions on another Machine reached through the control
- [ ] T4.3 Docs and help
- [ ] T4.4 Tried by hand in `just dev`

### T5. Keepers (was Track C)

- [ ] Owner answers the questions in `docs/R1_KEEPER_DESIGN.md` §13
- [ ] T5.1–T5.5 keeper per Session, reclaim at startup, updates no longer
      interrupt Sessions, restart and upgrade tests

### Also found, not yet scheduled

- `events`, `commands`, `metrics_minute`, and the control's notification
  tables are never pruned
- The projection recorder decodes every stored Session twice a second and
  re-records all of them at each start (`daemon/src/projection_runtime.rs`)
- The new `MetricReported` projection sends a program-printed name to the
  control; the projection otherwise carries no terminal text. Owner decides
  whether to allowlist names or keep metrics on the Machine
- Terminal input is written to the PTY on the async executor
  (`api.rs` → `write_input`); a program that does not read can block it
- 39 source files are over the 400-line rule
- Not yet read in the audit: most of `daemon/src/control`, `push`,
  `package_service`, the rest of the CLI, most of `web/src`, `scripts/`

---

## P: finish what is in progress

### P0. Commit the screen question work

Committed: `crates/watcher/src/screen_prompt.rs`
(questions read from the parsed screen), `Observation::screen`, daemon
`sample_screen` every 250 ms for busy FULL Sessions, prompt text fetched from the
owning Machine, `CodingAgent::{Vibe, Antigravity}`, option labels up to 160
characters, wider dashboard cards with an Open terminal button.

- [x] Committed to `dev`
- [ ] Owner retests with real `claude`, `vibe`, and `agy` in `just dev`

### P1. Answering Vibe's menu · done (arrows + Enter)

Vibe's menu says "arrows and Enter". Sentinal sends the option's digit, which is
unverified on real Vibe.

- [ ] Record what real Vibe does with a digit (replay a dev Session's log) — owner
- [x] For a Vibe Session, send arrow keys plus Enter, chosen from the Session's
      agent in `web/src/data/actions.ts` (`answerChoice`), never from screen text
- Where: `crates/watcher/src/screen_prompt.rs`, the daemon's prompt answer path

### P2. Menus without numbers · done

`menu_at` needs numbered rows (`1.` … `n.`). Some agents draw `❯ Yes` / `  No`.

- [x] Detect a marked list of 2–8 short rows under a question, only with a
      selection marker and a key hint, so plain text is never a menu
- [x] Answer by moving the marker (arrows) then Enter
- [x] Tests from real captured screens; a test that ordinary output is not a menu

### P3. Agent coverage

- [ ] Codex: capture a real approval screen and add a test (not installed on the
      owner's Mac yet)
- [ ] agy (Antigravity): capture a real permission dialog and add a test
- [ ] Gemini CLI: same
- [ ] Older control with a newer worker: check what happens with the new
      `CodingAgent` values. There is no `OTHER` value today; if an old control
      rejects the message, decide with the owner between adding a fallback value
      or leaving the field out for a control too old to know them (protocol
      change, so ask first)

### P4. Tool calls from Claude Code · done

Claude Code now prints `Running 1 shell command…` instead of `Bash(...)`, so
tool-call counts are 0.

- [x] Recognise the new wording (from screen rows, not only lines), keeping the
      old wording working
- Where: `crates/watcher/src/agent_cli.rs`

### P5. Name agent Sessions · done

An untitled Session shows as `Session 4`. Show the agent name instead
("Claude Code · project-folder") when the title is empty.

- [x] `sessionDisplayName` in `web/src/data/format.ts`, used by both
      `api/index.ts` and `api/projection-view.ts`

### P6. The `AI agents` filter · done

It decides by command name only. Also count a Session as an agent when its
screen matched an agent's prompt or banner. Explain the toggle in its tooltip.

- [x] Filter now also matches Sessions with `promptOptions` (a menu on screen)
- [x] Tooltip explains the toggle in `GlassDashboard.tsx`

---

## A: daily use

### A5. Automate notify.toml setup · new

Today the owner writes `~/.config/terminal-agent/notify.toml` by hand.
Add `sentinal notifications setup [--server URL] [--topic NAME]` (or fold
into `sentinal setup`) that creates the config dir, writes a valid
`notify.toml` with mode 0600, generates a random topic when absent, and
restarts the daemon. Refuses to overwrite an existing file without
`--force`. Test: fresh dir, existing file, bad server URL, permissions.

### A2. Every page has a way back · done

- [x] List every route in `web/src/routes/`; each has a visible way back or the
      places menu; add a browser test that walks them
- [x] Test in `navPlaces.test.ts`: every concrete route + detail routes resolve
      to a NAV_PLACE; projects now map to Overview

### A3. UI pass from real use

- [ ] With the owner: build a numbered problem list from `screenshot_issues/`
      (do not commit that folder) and new captures, into
      `docs/progress/ui-pass.md`
- [ ] Fix in the owner's priority order, one problem per change, each with a
      test where one is possible
- [x] 2026-11-16 (`f4ef4c9`): History rows space the command, capture
      badge and a styled Save as recipe button; Save as recipe uses the Input
      component, says why an ID is invalid and why an offline Machine cannot
      save; Diagnostics charts show one-sample series, have a median/p95 key,
      a styled Machine picker and ruled incident rows
- [ ] Known: question cards on the dashboard were hard to read (partly fixed by
      P0); notifications for questions sometimes never arrived; check this on
      production after v2.11.0, because dev never sends push

### A4. Hide and delete from the prompt · done

- [x] Prompt verbs `hide`, `restore`, `delete` for a Session (delete asks to
      confirm), using the existing `useSessionRemoval`

---

## B: watcher (progress without changing the program)

### B2. Progress API: one printed line · done

```text
::sentinal progress current=3 total=50 unit=task
::sentinal metric name=loss value=0.81
```

- [x] Parse these lines first, ahead of adapters and the fallback counter in
      `crates/watcher/src/progress.rs`; once a Session prints one, it is trusted
      over guesses for that Session
- [x] They are still untrusted output: they may only set progress and metrics,
      never run anything, change Session state, or notify. Bounds: numbers
      finite, `current <= total`, names ≤ 64 characters, rate limited like other
      progress
- [x] Malformed lines are ignored and shown as normal output
- [x] `docs/AGENT_GUIDE.md` (printed by `sentinal agent-guide`): copy-paste
      Python and shell examples
- [x] `sentinal help agents` prints the same guide (hidden `agents` alias)

### B1. Rules that recognise more

- [x] Explicit counters (`task 3/50`, `[4/10]`, tqdm bars) (committed in `02cea0a`)
- [x] `METRIC_REPORTED` Event from any program (loss, accuracy, tokens/s), not
      only Commands containing "train". Protocol change: fixtures
- [ ] Full-screen apps (nvtop, htop, vim): read the parsed screen on a timer,
      reusing the P0 screen sampling
- [x] Servers ("listening on :8080") without a dedicated adapter — generic
      adapter now emits `SERVER_STARTED` from common server-start lines
- [ ] Long runs: rate, estimated finish, time since last progress, and a
      progress chart on the Session page

### B3. Data collection for training

- [ ] Off by default, on per Machine, off per Session
- [ ] Sample only when the screen changed, at most once a minute per Session,
      duplicates dropped, a disk budget
- [ ] Remove secrets before writing; the owner reviews before any training
- [ ] Stored on that Machine, owner-only files; nothing leaves automatically
- [ ] Each sample stores what the rules decided, to find disagreements
- [ ] `sentinal data export` in the `eval/` format

### B4. Swappable models

- [ ] Optional step after the rules, off by default, per Machine: server URL,
      model name, prompt format. Everything works with no model
- [ ] Any OpenAI-compatible local server (a Mac mini running a bigger model)
- [ ] A prompt format for untuned models
- [ ] Screens are read on their own Machine and never go through the control
- [ ] Guards from `intelligence/` re-checked in the daemon; results marked
      "inferred"; inferred results never notify
- [ ] Compare models with `eval/score.py` on collected data

### B5. Fine-tune a small model · *after B3 has real labelled data*

- [ ] 100–200 hand-labelled real screens as the decision set
- [ ] Fine-tune FunctionGemma 270M or Qwen3-0.6B; score against rules and B4
- [ ] Default only after the gate in `docs/PLAN_V2_AI.md` §6.2

---

## C: multi-day runs survive a daemon restart · *now T5*

Today a daemon restart or update ends FULL Sessions. This track is built as
stage **T5** of the terminal upgrade package; the list below is kept for the
detail.

- [ ] Owner approves `docs/R1_KEEPER_DESIGN.md` (no code before this)
- [ ] A per-Session keeper keeps the PTY and log alive while `sentinald` restarts
- [ ] The restarted daemon reclaims its own keepers and rebuilds screen and
      tracker state; unknown or inconsistent keepers fail closed
- [ ] Upgrade and crash tests

## D: one install per Machine · done

All items done in `98f41d7` and `19be6fe`. Keep it that way: every new file that
holds credentials or settings must follow `config_dir`.

## R: release v2.11.0 · *owner only*

1. Owner has tried P0 in `just dev`
2. `just promote` (runs every check, merges `dev` into `main`, pushes nothing)
3. Owner: `sentinal backup create` on the control and on each worker (X15:
   2.10 cannot read what 2.11 stores, so the backup is the rollback)
4. Owner: `git checkout main && just ship 2.11.0`
5. Owner: `gh release edit v2.11.0 --notes-file docs/releases/v2.11.0.md`
   (the build leaves only the signing note as the release text, and controls
   link to that page)
6. Update `sentinal-control-one` (the control) **before** the workers, because
   of the protocol changes below

Full notes: `docs/releases/v2.11.0.md`.

### Protocol changes since v2.11.0

- `ATTACH.history` (optional, off by default; old daemons ignore it). New
  fixture entry in `client_requests.json`
- `WAITING_FOR_INPUT.marked` (optional, T0.8). New fixture entry (id 71) in
  `events.json`

### Release notes so far (protocol changes since v2.10.0)

- `RESTORE_SESSION`, `DELETE_SESSION`, `SESSIONS_DELETED`, tombstones
- Heartbeat `instance_id`, `build_commit`; `INSTANCE_CONFLICT`
- `CodingAgent::VIBE`, `ANTIGRAVITY`; prompt option labels up to 160 characters
- Store migrations 014 (`id_floors`) and 015 (`machine_instances`)
- `METRIC_REPORTED` EventKind + `ProjectedEventFacts::MetricReported` (B2/B1)
- Local `AGENT_STATE` (6.4) with `task`, `step`, `steps`, `eta_secs` (6.5);
  `agent_state` and `sandbox` on Session / ProjectedSession (never stored)
- `CREATE_SESSION.sandbox` (WATCH only)

## Other open items (older plans)

From `docs/ROADMAP.md` and `docs/AUDIT_TRACKER.md`; needs real Machines, so the
owner runs them:

- Two-real-Machine acceptance runs (4.A routing, 4.B demo)
- Real-phone acceptance and PWA push proof on a real iPhone
- Constant request counts at 10 Machines / 1,000 Sessions
- Installers for macOS and Linux with a verified fallback
- All `#[ignore]` integration tests on macOS and Linux hosts
- Agent tool API above level 0 (capped at Level 2) and assistant features: after
  everything above

## Open question for the owner

- Resolved 2026-11-16: four real Tailscale addresses (the pop-os worker, the
  Mac Mini control, the MacBook and the pilot VM) were in fixtures, tests and
  progress docs. They are now `100.64.1.10`–`100.64.1.13`. They remain in git
  history. Unsure, left as is: `100.101.29.67` (pairing tests) and
  `100.124.0.101` (`daemon/src/tailnet_dns.rs`, named Control-One); replace
  them too if they are real.
