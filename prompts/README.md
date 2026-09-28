# Build prompts

These prompts drive a coding agent (GPT-6-Astra or any other agentic coding
model) to build the whole system, one milestone at a time.

## How to use

1. Open a fresh agent session at the root of this repo, with shell access.
2. Paste **`00-master.md`**. The agent acknowledges and waits.
3. Paste the next milestone prompt, e.g. **`01-milestone-0-finish-protocol.md`**.
4. The agent works until the milestone is done, then stops with a report.
5. **You review** (checklist below), then paste the next milestone prompt.
   For long milestones, start a new session and paste `00-master.md` again
   first; the repo's docs and `docs/progress/` carry the memory.

| # | Prompt | Result |
|---|---|---|
| 0 | `01-milestone-0-finish-protocol.md` | Complete contract + TypeScript types |
| 1 | `02-milestone-1-persistent-sessions.md` | Detach/reattach from phone (v0.1) |
| 2 | `03-milestone-2-commands-store-metrics.md` | History, search, live resources (v0.2) |
| 1b.0 | `03a-milestone-1b0-contract-local-ingress.md` | Capture contract, capabilities, private local socket |
| 1b.A | `03b-milestone-1ba-sentinel-watch.md` | `sentinel watch`, durable full capture from any terminal |
| 1b.B | `03c-milestone-1bb-external-shell-hook.md` | Command/Metrics capture from ordinary shells |
| 1b.C | `03d-milestone-1bc-auto-wrap-adopt.md` | Fail-open auto-wrap and read-only process adoption |
| 3 | `04-milestone-3-events-watchers.md` | Progress, health, events without AI (v0.3) |
| 1b.D | `04a-milestone-1bd-adopted-log-ui.md` | Adopted log tail and capture-aware UI |
| 4 | `05-milestone-4-dashboard-recipes-notify.md` | Multi-machine, recipes, notifications (**V1**) |
| 5 | `06-milestone-5-projects-autocomplete.md` | Projects, command library, autocomplete |
| 6 | `07-milestone-6-intelligence-foundation.md` | Agent tool API, datasets, evals, baselines |
| 7 | `08-milestone-7-finetune-agent.md` | Fine-tuned small model, heartbeat, journal (**V2**) |

Milestone 1b.0 begins with a mandatory human review of the exact wire format,
Session termination semantics, and socket location. Paste the approval back
into the same agent session; do not let the agent infer those decisions.

## Review checklist (5 minutes per milestone)

- [ ] Run `just check` yourself. Does it pass on your machine?
- [ ] Try the "How to try it" steps from the report by hand.
- [ ] `git log` — small commits with clear messages?
- [ ] Any change under `crates/protocol/tests/fixtures/`? Was it intended?
- [ ] Were any tests deleted, weakened, or `#[ignore]`d? (`git diff --stat` on `tests/`)
- [ ] New dependencies justified in `docs/DEPENDENCIES.md`?
- [ ] Security: still no `0.0.0.0`, token required, audit log written?
- [ ] Answer the agent's "Questions for you" before the next milestone.

## If the agent goes off track

Paste: *"Stop. Re-read AGENTS.md and prompts/00-master.md. List which rule
you broke, revert that change, and continue from the last passing commit."*
