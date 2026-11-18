# Milestone 6.4 — Agent state, sandbox Sessions, and the agent skill

Report date: 2026-11-18. Tested on a working tree based on commit `e541476`
(uncommitted at the time of this report), macOS 27.0.1 (Apple silicon),
rustc 1.97.1, Node with pnpm, Playwright on Chromium.

Design and the owner's approval (all five decisions, 2026-11-17):
`docs/AGENT_STATE_DESIGN.md`. Prompt: `prompts/12-milestone-6d-agent-sandbox-state.md`.

## What was built

| Part | Where |
|---|---|
| `SENTINAL_AGENT_API=1` beside `SENTINAL_SESSION_ID` in every daemon-owned Session; daemon-owned, a client cannot set it | `crates/daemon/src/session_manager/lifecycle.rs`, `crates/protocol/src/session_env.rs` |
| `AGENT_STATE` local request (`WORKING`/`BLOCKED`/`WAITING`/`DONE`, detail ≤ 200 chars, no control characters). It carries **no process ID**: the socket layer reads the caller from OS credentials after decoding (`caller_pid`, `serde(skip)`) | `crates/protocol/src/{agent_state,message}.rs`, `crates/daemon/src/local_socket.rs` |
| Accepted only from a process inside that Session's process tree (OS peer PID → parent walk to `Session.pid`), only for live FULL Sessions, one per 500 ms per Session | `crates/daemon/src/session_manager/agent_state.rs`, `crates/daemon/src/process_probe.rs` |
| `agent_state` on `Session` and `ProjectedSession`; never written to the worker's store; cleared when the Command or Session ends (in `Session::transition`), kept while the agent asks a question | `crates/protocol/src/session.rs`, `crates/store/src/lib.rs` |
| Sandbox Sessions: `sentinal watch --sandbox [--cwd DIR]`; `CreateSession.sandbox` (WATCH only); private folder `<data_dir>/sandbox/<id>/` (0700) unless `--cwd`; kept after the Session ends; `sandbox` on `Session` and `ProjectedSession` | `crates/cli/src/{args,app,watch}.rs`, `crates/daemon/src/session_manager/lifecycle.rs` |
| `sentinal agent-state --status … [--detail …]`: silent, exit 0 outside a Session or when the daemon is down, exit 2 on a bad detail or status | `crates/cli/src/agent_state_command.rs` |
| `sentinal agent-skill` (alias `skill`) prints `docs/AGENT_SKILL.md`; `docs/AGENT_GUIDE.md` points to it | `crates/cli/src/lib.rs` |
| Web: "claude says · blocked · …" chip (solid outline = a claim, not a measurement) on dashboard rows, Needs-you rows, Session page and phone; a blocked agent shows in the island ticker without counting as a need; "Sandbox" badge with "Separate working folder. Still runs as you; not a security boundary." | `web/src/components/AgentStateChip.tsx`, `web/src/routes/app/shell/ticker.ts` |

Wire changes are additive; `PROTOCOL_VERSION` stays 1. New fixtures:
`AGENT_STATE` in `local_requests.json`, a sandbox `CREATE_SESSION` in
`client_requests.json`, and `session_agent_state.json`. `LocalResponse::Session`
now boxes its `Session` (clippy `large_enum_variant`), which does not change
the JSON.

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
136 Rust test suites passed, 0 failed; 529 web tests passed; 18 Playwright
specs passed (including the new `agent-state` project at 390×844). Two phone
terminal web tests time out at 5 s only while a Rust build runs in parallel;
they pass on a quiet machine.

## How "inside the Session" is proved

- `crates/daemon/tests/agent_state.rs` sends real framed requests over a
  Unix socketpair through `handle_connection`, so the OS reports this test
  process as the peer. A probe that places it under the Session's root is
  accepted; one that does not is `FORBIDDEN`, even though the request tries
  to name the root as its caller.
- `process_probe::tests::the_system_probe_sees_the_real_parent_of_this_process`
  exercises the real OS lookup (sysinfo) used in production.
- The e2e daemons run a fake echo PTY (`TA_FAKE_PTY=echo`), so no real
  process ever runs inside a Session there. `web/e2e/agent-state.spec.ts`
  therefore proves, at 390×844 on the real daemons: a sandbox Session gets
  `<data_dir>/sandbox/<id>`, the phone shows the Sandbox badge, and a report
  from the test process (real CLI, real socket, real OS credentials, but
  outside the Session) changes nothing and stays silent.

## Changes to existing tests (deliberate)

- `web/e2e/enrollment.spec.ts`, `web/e2e/mobile.spec.ts`: browser storage may
  also hold `sentinal.learn` and `sentinal.lifecycle` (learning tip, read and
  auto-clear choices) besides `sentinal.theme`. The allow-list stays exact
  and the no-secret checks are unchanged.
- `web/e2e/desktop-session.spec.ts`: at 768 px the Session page now stacks
  the terminal above the details (UX pass, 2026-11-17); 1024 px still checks
  two halves.
- `crates/cli/tests/agent_guide.rs`: a quoted argument in the guide is one
  word, as in a shell.

## Limits and follow-ups

- The positive path (a real agent in a real PTY reporting itself) has no
  browser acceptance run because the e2e harness has no real PTY. A manual
  check: inside any Sentinal terminal run
  `sentinal agent-state --status blocked --detail "manual check"` and watch the
  dashboard row.
- A sandbox is a separate folder and a label, not isolation.
- Milestone 6.5 (task, steps, ETA, Agents view) is next:
  `prompts/14-milestone-6e-agent-progress.md`.
