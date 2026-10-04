# Audit remediation tracker

This tracker rechecks the independent audit taken at commit `c89e78c` against
the repository after Milestones 4.A and 4.B. It records evidence, release
impact, and the next verifiable closure condition. Update it when a finding is
fixed; do not mark a row closed from a plan or an untested claim.

Last code review: **2026-10-04**. The recheck covered all 111 files changed
from audit baseline `c89e78c` through 4.B commit `9cb5a95`, plus every tracked
and newly added source/test/doc file in the pending telemetry and remediation
diff. Generated build/package/marketing artifacts and the owner's untracked
root files were not treated as project changes.

Status meanings:

- **Closed** — the implementation and a regression test are present.
- **Partial** — a material part landed, but the original risk still exists.
- **Open** — the audited behavior is still present.
- **Accepted** — a product judgement or consciously retained limitation, not
  a code defect that can be closed.

## Current position

| Status | Count | Findings |
|---|---:|---|
| Closed | 3 | F2, F6, F11 |
| Partial | 3 | F12, F14, F16 |
| Open | 11 | F1, F3, F5, F7–F10, F13, F15, F17–F18 |
| Accepted product risk | 1 | F4 |

Counting a partial finding as one half, **4.5 of 17 remediable findings (26%)**
are closed. This number measures the audit backlog, not implementation progress
through the roadmap.

**V1 judgement — high confidence:** the feature implementation is at the V1
boundary, but the project is not yet a releasable V1. The release gates are
browser credential transport (F3), credential refresh recovery (F5), bounded
sensitive storage (F9), login throttling
(F10), a production PWA delivery path (N1), a green integration suite (F14),
and the recorded two-real-Machine 4.A/4.B acceptance. F2 is closed by the
pending audit-hardening commit. F1 may remain in R1 only if every V1 claim says
that Sessions survive client disconnects but not a daemon restart.

## Finding tracker

| ID | Severity | Status | Current evidence | V1 disposition | Closure condition |
|---|---|---|---|---|---|
| F1 | High | Open | `crates/daemon/src/entry.rs` calls `interrupt_orphaned_sessions`; `docs/ROADMAP.md` keeps the PTY keeper in R1. | Scope gate | Either deliver R1, or make the daemon-restart limit explicit in every V1/product surface and remove any stronger persistence claim. |
| F2 | High | Closed | `crates/protocol/src/recipe.rs` accepts placeholders only as standalone unquoted shell words, rejects quoted/adjacent contexts, rejects control characters, and tests the original `$()` exploit. Rendering still occurs in the daemon through `Recipe::render`. | Must remain closed | Protocol and fake-PTY injection tests pass; no alternative rendering path exists. Prefer executable/argv recipes in the next contract revision. |
| F3 | High | Open | `web/src/data/api/auth.ts` stores the 24-hour bearer in `localStorage`; `web/src/data/api/index.ts` puts it in the WebSocket `token` query. | Release blocker | Use a Secure, HttpOnly, SameSite owner cookie or a short-lived, one-use WebSocket ticket; add log/referrer/storage tests and recent authentication for destructive actions. |
| F4 | High | Accepted | The 2026-10-03 market review found strong terminal/phone alternatives. 4.A/4.B do not change that market fact. | Product validation | Measure use of structured cross-Machine events and attention detection. Recheck competitors quarterly; do not count remote terminal access alone as differentiation. |
| F5 | Medium | Open | `ControlService::refresh` writes `machine-credentials.json` before `Store::rotate_link`; the file and SQLite changes have no shared journal, and the returned `bool` is ignored. | Release blocker | Add an intent journal/startup reconciliation, make refresh idempotent, check that exactly one link row rotated, and inject failures at each boundary. |
| F6 | Medium | Closed | `CommandTracker::finish_on_shell_exit` finishes the current Command with the child exit code before the Session reaches its terminal state. Tracker and stored-Session tests cover an unterminated training command and a shell exiting at an idle prompt. | Must remain closed | Keep the command update, finish Event, Session transition, and persistence ordered under injected-time tests. |
| F7 | Medium | Open | `Envelope::decode` still requires exact `PROTOCOL_VERSION == 1`; link HTTP payloads have no negotiated version. | Before public compatibility promise | Negotiate major/minimum version and capabilities, with mixed control/worker tests and explicit link API versioning. |
| F8 | Medium | Open | Rust wire IDs remain `u64`; generated TypeScript remains `number` (`TS_RS_LARGE_INT=number`). | Before durable/public protocol promise | Serialize IDs as decimal strings or reject allocation before JavaScript's safe-integer limit; migrate fixtures deliberately. |
| F9 | Medium | Open | Metric retention and in-memory scrollback are bounded, but compressed Session logs and JSONL audit/link logs have no total quota, age rotation, or purge command. | Release blocker | Add per-Session/global byte and age limits, visible usage, rotation, and an exact-path purge flow; test disk-full behavior. |
| F10 | Medium | Open | In Serve mode `AppState::client_key` trusts a loopback `Tailscale-User-Login` value as the only password-throttle key. A local process can vary it. | Release blocker | Add a global secondary attempt budget or authenticate the proxy-to-daemon identity channel; test spoofed rotating headers. |
| F11 | Medium | Closed | `crates/daemon/src/control/proxy.rs::prove_identity` authenticates `/api/v1/link/identity` and pins the returned immutable Machine ID before both WebSocket and output proxying. 4.A adapter/E2E tests cover credential and frame isolation. | Must remain closed | Keep identity proof mandatory on every routed transport and preserve the two-worker isolation tests. |
| F12 | Medium | Partial | The `gpu_smi` fallback gives static Linux builds NVIDIA inventory/metrics. `crates/watcher/src/generic.rs` still treats missing GPU as zero and has no sleep/wake reset; Apple GPU remains unavailable. | Platform claim gate | Reset quiet clocks across sleep/wake, require active work before stall events, represent unavailable GPU distinctly, and document/test macOS behavior. |
| F13 | Medium | Open | Nonce-tagged OSC and bash/zsh/fish hooks remain sound, but there is no explicit compatibility matrix for Powerlevel10k, heredocs, nested shells, SSH/tmux, or VS Code OSC 633. | Platform claim gate | Add real-shell ignored integration cases and document intentionally opaque nesting. |
| F14 | Medium | Partial | `adopt_command::candidates` parses portable `ps etime`, ranks newest first, then truncates. A regression with 45 old high-PID processes and one new PID 42 failed before the fix and passes now; the real adoption integration also passes on macOS. The aggregate ignored command now gets past adoption and stops only at the intentionally unconfigured real-Tailnet enrollment test. | Release/platform evidence | Run the host-local ignored tests as an explicit suite on Linux; keep real-Tailnet/manual benchmarks separate so missing external credentials do not hide local integration results. |
| F15 | Medium | Open | `every_event_kind_has_a_fixture` still compares fixtures with a manually maintained string array, so a new enum variant can evade it. | Quality gate | Derive coverage from an authoritative `EventKind` variant list/schema or an exhaustive compile-time match; repeat the mutation. |
| F16 | Medium | Partial | `eval/` adds deterministic generation, held-out format families, natural negatives, planted secrets, precision/recall/F1, grounding and leak metrics; `watcher/examples/eval_baseline.rs` provides the rules baseline. On 2026-10-04 the deterministic baseline scored F1 0.317 (precision 0.661, recall 0.209), 22.8% exact match, zero secret-leak examples, and 95.9% clean negatives on 1,528 generated/real-log cases. No model result or operational win exists yet. | V2 only | Freeze a hand-reviewed test subset, hold out by project/machine/time, report confidence intervals/calibration, and fine-tune only after a prompted 4–8B baseline shows a useful gap. |
| F17 | Low | Open | Security-sensitive files still exceed the approximately 400-line rule: `control/mod.rs` 893, `registry.rs` 672, `protocol/link.rs` 619, `link/mod.rs` 597, and `connect_command.rs` 485 lines. | Post-V1 maintainability | Split by responsibility without behavior changes after the release blockers are closed. |
| F18 | Low | Open | Heartbeat skew uses signed subtraction before `abs`; `ControlClient` silently falls back after secure client-builder failure; credential renames fsync files but not parent directories. | Hardening before 1.0 | Use overflow-safe difference, return client construction errors, fsync parent directories, and fault-test exceptional paths. |

## New finding from the recheck

| ID | Severity | Area | Finding and evidence | Impact | Closure condition |
|---|---|---|---|---|---|
| N1 | High | Delivery | `docs/SETUP.md` states that only the Vite development server is shipped, and `scripts/run-control.sh` starts Vite directly. There is no production-built PWA service behind the control origin. | The phone workflow is implemented but cannot yet be installed and operated as a normal V1 release. | Package the built app shell, serve it from the authenticated control origin, document upgrade/rollback/cache behavior, and repeat the 4.B phone acceptance against that build. |

## Documentation and plan contradictions

| ID | Status | Contradiction | Required correction |
|---|---|---|---|
| C1 | Open | The audit-requested `docs/report-additions/SENTINEL-PLAN-updated.txt` does not exist; `docs/PLAN.md` is the active plan. | Remove the stale reference wherever the audit prompt is retained, or add an explicit pointer file. |
| C2 | Open | `docs/PLAN.md` says capture-mode part D is pending; `docs/ROADMAP.md` marks adopted-log ingestion complete. | Update the plan status. |
| C3 | Open | `docs/SETUP.md` says the watcher and rich health are later work; Roadmap M3 and the code say they are complete. | Replace the stale current-boundary section. |
| C4 | Open | `docs/progress/milestone-4.0.md` says both link credentials rotate atomically; the implementation writes a JSON file and SQLite separately. | Describe the actual failure boundary until F5 closes. |
| C5 | Open | `docs/SECURITY.md` says every route except health requires a bearer; setup/login/status and pairing exchange intentionally use other rules. | List public, bootstrap, owner, worker, and control route classes explicitly. |
| C6 | Open | `docs/SECURITY.md` says throttling is per client address, while Serve mode keys it by a forwarded login header. | Document both keys and the F10 residual local-user risk. |
| C7 | Open | `just check` still excludes Playwright, although 4.A/4.B definitions of done require browser acceptance. | Add an explicit CI/acceptance target that runs both fast checks and Playwright, while keeping the fast local target clear. |
| C8 | Open | The old Milestone 1 prompt names `dev.terminal-agent.tad.plist`; current packaging uses the Sentinal service name while retaining legacy compatibility. | Mark the old path historical or update the prompt. |
| C9 | Open | Older progress reports contain historical test counts that no longer describe HEAD. | Label counts with commit IDs and keep only the current count in this tracker. |
| C10 | Partial | Endpoint identity verification is mandatory (F11 closed), and Recipe context validation closes F2. Credential crash recovery and the `just check`/Playwright distinction remain likely agent failure points. | Close F5 and C7, then remove this row. |

## Verification record

| Check | 2026-10-04 result |
|---|---|
| `just check` | Pass: format, all-feature Clippy with warnings denied, workspace tests, TS binding check, 30 web files / 140 tests, 8 evaluator tests, and all shell checks. |
| `just test-e2e` | Pass: 5 Playwright tests, including two workers and the 390×844 mobile Recipe flow. |
| `cargo doc --workspace --no-deps` | Pass without warnings after fixing the stale `<tailnet>` rustdoc markup. |
| Adoption mutation/regression | The new low-PID process test failed against PID sorting and passes after elapsed-time sorting. |
| Real adoption integration | Pass on macOS outside the filesystem sandbox. |
| Full `--ignored` aggregate | Not fully run: adoption and auto-wrap passed; execution stopped at the real-Tailnet enrollment test because `SENTINAL_ENROLL_CONTROL_URL` was intentionally not supplied. This is an external acceptance prerequisite, not a product test failure. |
| Deterministic translator baseline | 1,528 cases: F1 0.317, precision 0.661, recall 0.209, exact match 22.8%, zero secret-leak examples, clean negatives 95.9%. |

The market-risk finding remains current as of 2026-10-04. Official Zellij
documentation still describes authenticated HTTPS browser access, HttpOnly
session cookies, read-only tokens, a PWA, and a dedicated mobile UI
([source](https://zellij.dev/documentation/web-client.html)). Wave still
documents SSH job managers that survive client restarts and sleep
([source](https://docs.waveterm.dev/durable-sessions)). These checks reinforce
F4: structured cross-Machine interpretation is the credible wedge.

## V1 release checklist

The project may be labelled **V1 release candidate** when every item below is
checked. It may be labelled **V1** after the two real-Machine run passes on the
candidate build.

- [x] F2 recipe-context regression is committed and `just check` passes.
- [ ] F3 browser/WS bearer exposure is removed.
- [ ] F5 credential refresh is crash-recoverable and idempotent.
- [x] F6 a shell exit cannot leave a RUNNING Command.
- [ ] F9 storage has enforceable limits and purge behavior.
- [ ] F10 login has a non-spoofable/global secondary throttle.
- [ ] F14 all ignored integration tests pass on the supported macOS and Linux hosts.
- [ ] N1 the production PWA is packaged behind the control origin.
- [ ] The exact 4.A and 4.B two-real-Machine acceptance runs are appended to
      their progress reports, including OS versions and tested commit.
- [ ] The release notes state the F1 daemon-restart limit, or R1 is complete.

## Next remediation order

1. Commit and verify F2; keep remote Recipe execution blocked on any validation
   failure.
2. Replace the browser bearer/query-token design (F3).
3. Make credential refresh recoverable and idempotent (F5).
4. Reconcile active Commands on child exit (F6).
5. Add storage quotas, rotation, usage, and purge (F9).
6. Add the global/non-spoofable login throttle (F10).
7. Ship the production PWA path (N1).
8. Fix the adoption picker and run the full ignored suite (F14).
9. Run and record the real 4.A/4.B acceptance on the release candidate.
10. Address protocol evolution/ID representation (F7/F8) before calling the
    wire format stable, then schedule R1 from actual user evidence.
