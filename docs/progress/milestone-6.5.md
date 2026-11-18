# Milestone 6.5 — Agent task progress and the Agents view

Report date: 2026-11-18. Tested on a working tree based on commit `996dd3c`,
macOS 27.0.1 (Apple silicon), rustc 1.97.1, Node with pnpm, Playwright on
Chromium.

Design: `docs/AGENT_STATE_DESIGN.md` §6.5 and the review "6.5 review against
the code after 6.4"; all seven changes approved by the owner on 2026-11-18.
Prompt: `prompts/14-milestone-6e-agent-progress.md`. Design board: B2.

## What was built

| Part | Where |
|---|---|
| Optional `task` (≤ 80 chars), `step`/`steps` (`step ≤ steps ≤ 10 000`), `eta_secs` (≤ 7 days) on `AGENT_STATE` and `AgentReportedState`; `parse_agent_eta` (`90s`, `12m`, `2h`) | `crates/protocol/src/agent_state.rs`, `message.rs` |
| Daemon-only `task_started_at` and `estimated_left_secs`: never read from a request | `crates/daemon/src/session_manager/agent_state.rs` |
| A report keeps the progress it leaves out; `status`/`detail` replaced; a new task starts fresh; the agent's ETA stands only while the step is unchanged; a step past the plan after merging is refused | same |
| Pace estimate: median time per step over the last six step times × steps left; absent with one step; reset on a new task, a changed plan, a step going backwards, or when the Command ends | same |
| The 500 ms limit holds the newest early report and applies it when the window ends (CLI: timer; script line: next output), so a quick last step or `done` is never lost | same, `tasks.rs` |
| `::sentinal task name=… [step=N steps=M] [eta=12m]` and `::sentinal state value=…` lines: returned as `EngineUpdate.agent_line`, never an Event, applied through the same path | `crates/watcher/src/{progress,engine}.rs`, `crates/daemon/src/session_manager/tasks.rs` |
| `sentinal agent-state … --task … --step N --of M --eta 12m`; same silence and exit codes | `crates/cli/src/agent_state_command.rs` |
| `sentinal agent-skill --install [--file F]`: prints the exact diff, writes only after `y` on a real terminal, writes nothing without one (exit 1), no `--yes`, changes only the marked section | `crates/cli/src/skill_install.rs` |
| Skill and guide text for both paths | `docs/AGENT_SKILL.md`, `docs/AGENT_GUIDE.md` |
| Web: Agents view at `/app/agents` (places menu, prompt word `agents`, phone More → Places, phone overview link): grouped by Project then folder, filters All / Needs you / Working / Done, task, agent, Machine, state, steps, "claude says ~12 m", "about 15 m at this pace", time on task, inline answers | `web/src/routes/app/AgentsPage.tsx` |
| Island ring and dashboard bar from a working agent's steps, outlined and labelled "claude says · step 3 of 8" | `web/src/routes/app/shell/{ticker.ts,LiveIsland.tsx}`, `dashboard/GlassDashboard.tsx` |
| Settings → Agents renamed **Agent access** (search word `agent access`) | `web/src/routes/app/settings-tabs.ts`, `AgentsTab.tsx`, `phone/SettingsHome.tsx` |

No new notification kinds. Reported progress never changes lifecycle state
and is never written to the store. Wire changes are additive;
`PROTOCOL_VERSION` stays 1. New fixtures: a progress `AGENT_STATE` in
`local_requests.json` and `session_agent_state_progress.json`.

## Commands and results

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
just gen-ts --check
cargo test --workspace --no-fail-fast
pnpm -C web check
pnpm -C web test:e2e
```

Results (2026-11-18): format and clippy clean; generated TypeScript current;
136 Rust test suites passed, 0 failed; 543 web tests passed; 19 Playwright
specs passed, including the new 6.5 test in the `agent-state` project. The
web server printed harmless `write EPIPE` lines when browsers closed
connections early; no test failed.

## The "done when" check

`web/e2e/agent-state.spec.ts` (second test) runs on the real daemons. The
e2e PTY echoes what is typed, so typing
`::sentinal task name="migrate tests" step=3 steps=8 eta=12m` produces real
Session output. Within one second the Session page shows "agent says ·
working · migrate tests · step 3 of 8"; the Agents row shows "step 3 of 8 ·
agent says ~12 m · … on task · 38 %" at 1280 px and again at 390 px; no
stored Event contains the task. The island ring for agent steps is covered by
`shell.test.ts` (38 %, "claude says · step 3 of 8"), since other e2e specs
may leave Sessions that need the owner, which take the island first.

`sentinal agent-state` itself, from a process inside a real Session, is
proved by `crates/daemon/tests/agent_state.rs` over a real socketpair (as in
6.4). Manual check inside any Sentinal terminal:

```sh
sentinal agent-state --status working --task "migrate tests" --step 3 --of 8 --eta 12m
```

## Changes to existing tests (deliberate)

- `web/e2e/agents.spec.ts`: the Settings tab heading is now "Agent access".
- `crates/daemon/tests/agent_state.rs`: the 6.4 rate-limit test still
  passes unchanged; an early report is now held and applied later rather
  than discarded.
- `crates/protocol/tests/agent_state.rs`, `projection.rs`,
  `crates/store/tests/store.rs`: constructors gained the new optional fields.

## Limits

- Time on task uses the browser clock against the daemon's timestamp, so a
  large clock difference between machines shifts it.
- Sentinal's pace estimate is computed when a report arrives and is not
  counted down between reports.
