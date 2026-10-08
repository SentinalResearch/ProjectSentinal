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
| 1b.A | `03b-milestone-1ba-sentinal-watch.md` | `sentinal watch`, durable full capture from any terminal |
| 1b.B | `03c-milestone-1bb-external-shell-hook.md` | Command/Metrics capture from ordinary shells |
| 1b.C | `03d-milestone-1bc-auto-wrap-adopt.md` | Fail-open auto-wrap and read-only process adoption |
| 1b.E | `03e-milestone-1be-finished-session-replay.md` | Read-only finished Sessions, saved-output replay, safe Continue |
| 1c | `03f-milestone-1c-worker-packaging-two-host.md` | Installable one-worker/two-host Tailscale acceptance |
| 1c.1 | `03h-milestone-1c1-tailnet-bootstrap.md` | Tailnet curl bootstrap and private verified package delivery |
| 1d | `03g-milestone-1d-session-continuation-history.md` | Persistent continuation lineage and bounded combined output history |
| 3 | `04-milestone-3-events-watchers.md` | Progress, health, events without AI (v0.3) |
| 1b.D | `04a-milestone-1bd-adopted-log-ui.md` | Adopted log tail and capture-aware UI |
| 4.0 | `05a-milestone-40-secure-machine-enrollment.md` | Threat-reviewed pairing and per-Machine credentials |
| 4.A | `05b-milestone-4a-multi-machine-aggregation.md` | Direct multi-Machine aggregation and safe routing |
| 4.B | `05-milestone-4-dashboard-recipes-notify.md` | Recipes and notifications complete **V1** |
| 4.C | `05c-milestone-4c-notification-mobile-acceptance.md` | Real ntfy delivery, production PWA, phone appearance and navigation |
| 4.D | `05d-milestone-4d-native-push-gateway.md` | Control-owned Web Push now, provider boundary for future native apps |
| 4.E | `05e-milestone-4e-v1-install-discovery-private-pilot.md` | Normal-person install, Tailnet discovery, approval pairing, private cloud pilot |
| 4.F | `05f-milestone-4f-mobile-web.md` | Mobile web experience from `sentinal-mobile-handoff/`: tab bar, phone screens, terminal, sheets |
| 5 | `06-milestone-5-projects-autocomplete.md` | Projects, command library, autocomplete |
| 6 | `07-milestone-6-intelligence-foundation.md` | Agent tool API, datasets, evals, baselines |
| 7 | `08-milestone-7-finetune-agent.md` | Fine-tuned small model, heartbeat, journal (**V2**) |
| R1 | `09-reliability-r1-daemon-restart-sessions.md` | Optional keeper architecture for PTYs across daemon restart |

Milestones 1b.0, 4.0, and 4.D, plus Reliability R1, begin with a mandatory
human review. Paste the approval back into the same agent session; do not let
the agent infer wire, pairing, push-security, dependency, or PTY-ownership
decisions.

The recommended path from the current repository is 1b.E → 1c → 1c.1 → 1d →
3 → 1b.D → 4.0 → 4.A → 4.B → 4.C → 4.D → 4.F → 4.E. 4.F comes before 4.E so
the clean-install pilot and real-phone acceptance exercise the phone screens. R1 is a separate reliability project:
schedule it before a production-stability claim, but do not mix it into
watcher or dashboard work.

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
