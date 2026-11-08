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

### P1. Answering Vibe's menu

Vibe's menu says "arrows and Enter". Sentinal sends the option's digit, which is
unverified on real Vibe.

- [ ] Record what real Vibe does with a digit (replay a dev Session's log)
- [ ] If digits do not work, send arrow keys plus Enter for that agent, chosen
      in the daemon, never from screen text
- Where: `crates/watcher/src/screen_prompt.rs`, the daemon's prompt answer path

### P2. Menus without numbers

`menu_at` needs numbered rows (`1.` … `n.`). Some agents draw `❯ Yes` / `  No`.

- [ ] Detect a marked list of 2–8 short rows under a question, only with a
      selection marker and a key hint, so plain text is never a menu
- [ ] Answer by moving the marker (arrows) then Enter
- [ ] Tests from real captured screens; a test that ordinary output is not a menu

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

### P4. Tool calls from Claude Code

Claude Code now prints `Running 1 shell command…` instead of `Bash(...)`, so
tool-call counts are 0.

- [ ] Recognise the new wording (from screen rows, not only lines), keeping the
      old wording working
- Where: `crates/watcher/src/agent_cli.rs`

### P5. Name agent Sessions

An untitled Session shows as `Session 4`. Show the agent name instead
("Claude Code · project-folder") when the title is empty.

### P6. The `AI agents` filter

It decides by command name only. Also count a Session as an agent when its
screen matched an agent's prompt or banner. Explain the toggle in its tooltip.

---

## A: daily use

### A2. Every page has a way back

- [ ] List every route in `web/src/routes/`; each has a visible way back or the
      places menu; add a browser test that walks them

### A3. UI pass from real use

- [ ] With the owner: build a numbered problem list from `screenshot_issues/`
      (do not commit that folder) and new captures, into
      `docs/progress/ui-pass.md`
- [ ] Fix in the owner's priority order, one problem per change, each with a
      test where one is possible
- [ ] Known: question cards on the dashboard were hard to read (partly fixed by
      P0); notifications for questions sometimes never arrived; check this on
      production after v2.11.0, because dev never sends push

### A4. Hide and delete from the prompt

- [ ] Prompt verbs `hide`, `restore`, `delete` for a Session (delete asks to
      confirm), using the existing `useSessionRemoval`

---

## B: watcher (progress without changing the program)

### B2. Progress API: one printed line · *do this next*

```text
::sentinal progress current=3 total=50 unit=task
::sentinal metric name=loss value=0.81
```

- [ ] Parse these lines first, ahead of adapters and the fallback counter in
      `crates/watcher/src/progress.rs`; once a Session prints one, it is trusted
      over guesses for that Session
- [ ] They are still untrusted output: they may only set progress and metrics,
      never run anything, change Session state, or notify. Bounds: numbers
      finite, `current <= total`, names ≤ 64 characters, rate limited like other
      progress
- [ ] Malformed lines are ignored and shown as normal output
- [ ] `docs/AGENT_GUIDE.md` (printed by `sentinal agent-guide`): copy-paste
      Python and shell examples
- [ ] `sentinal help agents` prints the same guide

### B1. Rules that recognise more

- [x] Explicit counters (`task 3/50`, `[4/10]`, tqdm bars) (committed in `02cea0a`)
- [ ] `METRIC_REPORTED` Event from any program (loss, accuracy, tokens/s), not
      only Commands containing "train". Protocol change: fixtures
- [ ] Full-screen apps (nvtop, htop, vim): read the parsed screen on a timer,
      reusing the P0 screen sampling
- [ ] Builds, test runs, downloads, servers ("listening on :8080") without a
      dedicated adapter
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

## C: multi-day runs survive a daemon restart

Today a daemon restart or update ends FULL Sessions.

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
3. Owner: `git checkout main && just ship 2.11.0`
4. Update `sentinal-control-one` (the control) **before** the workers, because
   of the protocol changes below

### Release notes so far (protocol changes since v2.10.0)

- `RESTORE_SESSION`, `DELETE_SESSION`, `SESSIONS_DELETED`, tombstones
- Heartbeat `instance_id`, `build_commit`; `INSTANCE_CONFLICT`
- `CodingAgent::VIBE`, `ANTIGRAVITY`; prompt option labels up to 160 characters
- Store migrations 014 (`id_floors`) and 015 (`machine_instances`)

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

- `crates/protocol/tests/fixtures/daemon_messages.json` contains an address in
  the 100.x range. If it is a real Tailscale address, replace it with a
  documentation address (`100.64.0.1`) and update the fixture.
