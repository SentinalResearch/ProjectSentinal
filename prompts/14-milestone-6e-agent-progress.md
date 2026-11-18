# MILESTONE 6.5 — Agent task progress and the agents view (V2 part 5)

Goal: every AI coding agent running inside a Sentinal Session (Claude Code,
Codex, Antigravity, Gemini, Aider, or the owner's own script) shows the owner
**what task it is on, how far along it is, and roughly how long is left**, on
desktop and phone, without anyone reading its screen. One "Agents" view lists
every agent on every Machine, grouped by Project, with done / working /
needs-you at a glance.

Inspired by Herdr's per-agent status and time-on-task
(https://herdr.dev), adapted to Sentinal's security model. Design boards:
`B2 · Agents grouped by space` on the Sentinal design canvas.

Depends on Milestone 6.4 (`AGENT_STATE` over the local socket,
`SENTINAL_SESSION_ID`, `sentinal agent-state`, `docs/AGENT_SKILL.md`) and
6.1 (agent-aware Sessions). Read `AGENTS.md`, `docs/AGENT_STATE_DESIGN.md`,
`docs/AGENT_GUIDE.md`, `crates/watcher/src/progress.rs`, and
`crates/watcher/src/agent_cli.rs` before changing code.

No model makes decisions in this milestone. Terminal output stays untrusted.

## Required design review — stop before tests or implementation

`docs/AGENT_STATE_DESIGN.md` §6.5 is the proposal. Confirm it still matches
the code after 6.4 landed, list any change, and stop for explicit approval.

## What to build after approval

### 1. Progress on `AGENT_STATE` (additive)

- Optional fields on the 6.4 request and on `AgentReportedState`:
  `task` (≤ 80 chars, no control characters), `step` and `steps`
  (non-negative integers, `step <= steps`, `steps <= 10_000`), and
  `eta_secs` (≤ 7 days). Each is independently optional.
- `sentinal agent-state --status working --task "migrate tests" --step 3 --of 8 [--eta 12m]`.
  `--eta` accepts `90s`, `12m`, `2h`. Same silence and exit-code rules as 6.4.
- The daemon records `started_at` for the current `task` (it resets when
  the task text changes) so every surface can show time on task.
- Sentinal's own estimate: when `step`/`steps` are known, the daemon keeps
  the last five step timestamps and computes `estimated_left_secs` from the
  median step time. The agent's `eta_secs` and Sentinal's estimate are kept
  apart and labelled differently ("claude says ~12 m" vs "about 15 m at this
  pace"). Neither is ever shown as known fact.
- Reported progress never changes lifecycle state and never notifies on its
  own; `DONE` from an agent is shown, but Session state still comes from the
  watcher.

### 2. `::sentinal` line parity for scripts

- Extend the existing explicit line API (`crates/watcher/src/progress.rs`)
  with `::sentinal task name=… [step=N steps=M] [eta=12m]` and
  `::sentinal state value=working|blocked|waiting|done`, so a plain script
  that prints to its own terminal gets the same display without the CLI.
  Same bounds, same "malformed line is ignored and shown as output" rule.
- Document both paths in `docs/AGENT_GUIDE.md`: the CLI for coding agents
  (their tool output may never reach the screen), the line for scripts.

### 3. Agent skill text

- Extend `docs/AGENT_SKILL.md` (6.4) with one short section: when a task
  will take more than a few minutes, report the plan's step count at the
  start, update after each step, give an honest `--eta` or none, and report
  `blocked` with the reason when waiting on the owner.
- `sentinal agent-skill --install` writes or updates a clearly delimited
  section in the current project's `AGENTS.md` / `CLAUDE.md` only after the
  owner confirms the exact diff; never silently.

### 4. Web UI

- **Agents view** (`/app/agents`, in the places menu and prompt as `agents`):
  every Session with `coding_agent` or `agent_state`, grouped by Project
  (falling back to cwd), filters All / Needs you / Working / Done. Each row:
  task, agent, Machine, state, `step of steps`, time on task, the two ETAs
  labelled as above. A waiting agent keeps its inline answers (6.1).
- **Island and dashboard:** a working agent with steps shows a progress ring
  like a training run, with "claude says" in its label.
- **Phone:** the same rows in the quiet phone style.
- Reported values use a "reported" marker (solid outline), distinct from
  known values (no marker) and model guesses (dashed, 6.2).

### 5. Notifications

- No new notification kinds. An agent reporting `blocked` already maps to
  the existing WAITING rules only when the watcher also sees a question;
  otherwise it appears in the inbox as information.

## Tests first

- Protocol: legacy `AGENT_STATE` without the new fields decodes; bounds on
  every field; fixtures round-trip; TypeScript current.
- Daemon: task change resets `started_at`; step history → estimate with an
  injected clock; estimate absent with fewer than two steps; reported fields
  clear when the Command finishes.
- Watcher: `::sentinal task` / `state` lines parse, bounds, malformed lines
  ignored, never change lifecycle state.
- CLI: `--step/--of/--eta` parsing and errors; silence rules unchanged.
- Web: agents view grouping and filters; ETA labels; reported vs guessed
  markers; island ring for agent steps; phone rows.

## Done when

Inside a Sentinal Session, Claude Code runs
`sentinal agent-state --status working --task "migrate tests" --step 3 --of 8 --eta 12m`
and within one second the owner sees, on desktop and phone, an Agents row
"migrate tests · step 3 of 8 · claude says ~12 m · 4 m on task", and an
island ring at 38 %. `just check` and `just test-e2e` pass, fixtures are
updated deliberately, Roadmap 6.5 is ticked, and the report is sent.
Commit and stop.
