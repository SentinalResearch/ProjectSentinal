# Audit remediation tracker

This tracker rechecks the independent audit taken at commit `c89e78c` against
the repository after Milestones 4.A and 4.B. It records evidence, release
impact, and the next verifiable closure condition. Update it when a finding is
fixed; do not mark a row closed from a plan or an untested claim.

Last code review: **2026-10-04**. The recheck covered all 111 files changed
from audit baseline `c89e78c` through 4.B commit `9cb5a95`, plus every tracked
and newly added source/test/doc file in audit-remediation commit `cd38af6`.
Generated build/package/marketing artifacts and the owner's untracked root
files were not treated as project changes.

Status meanings:

- **Closed** — the implementation and a regression test are present.
- **Partial** — a material part landed, but the original risk still exists.
- **Open** — the audited behavior is still present.
- **Accepted** — a product judgement or consciously retained limitation, not
  a code defect that can be closed.

## Current position

| Status | Count | Findings |
|---|---:|---|
| Closed | 6 | F2, F3, F6, F11, F19, F20 |
| Partial | 3 | F12, F14, F16 |
| Open | 10 | F1, F5, F7–F10, F13, F15, F17–F18 |
| Accepted product risk | 1 | F4 |

Counting a partial finding as one half, **7.5 of 19 remediable findings (39%)**
are closed. This number measures the audit backlog, not implementation progress
through the roadmap.

**V1 judgement — high confidence:** the feature implementation is at the V1
boundary, but the project is not yet a releasable V1. The release gates are
credential refresh recovery (F5), bounded sensitive storage (F9), login
throttling (F10), a green integration suite (F14),
and the recorded two-real-Machine 4.A/4.B acceptance. F2 and F6 are closed in
`cd38af6`. F1 may remain in R1 only if every V1 claim says that Sessions survive
client disconnects but not a daemon restart.

## Finding tracker

| ID | Severity | Status | Current evidence | V1 disposition | Closure condition |
|---|---|---|---|---|---|
| F1 | High | Open | `crates/daemon/src/entry.rs` calls `interrupt_orphaned_sessions`; `docs/ROADMAP.md` keeps the PTY keeper in R1. | Scope gate | Either deliver R1, or make the daemon-restart limit explicit in every V1/product surface and remove any stronger persistence claim. |
| F2 | High | Closed | `crates/protocol/src/recipe.rs` accepts placeholders only as standalone unquoted shell words, rejects quoted/adjacent contexts, rejects control characters, and tests the original `$()` exploit. Rendering still occurs in the daemon through `Recipe::render`. | Must remain closed | Protocol and fake-PTY injection tests pass; no alternative rendering path exists. Prefer executable/argv recipes in the next contract revision. |
| F3 | High | Closed | Browser auth uses a Secure, HttpOnly, SameSite=Strict owner cookie in production; web code receives no bearer, terminal WebSocket URLs contain no token, production rejects query-token auth, and tests sweep storage/URLs. | Must remain closed | Keep browser credentials out of JavaScript, URLs, referrers, logs, and Cache Storage; retain cookie/logout/WS regression tests. |
| F4 | High | Accepted | The 2026-10-03 market review found strong terminal/phone alternatives. 4.A/4.B do not change that market fact. | Product validation | Measure use of structured cross-Machine events and attention detection. Recheck competitors quarterly; do not count remote terminal access alone as differentiation. |
| F5 | Medium | Closed | `control/refresh.rs`: the secrets file records a `pending_refresh` (the previous presented credential, worker hash, and expiry) atomically with the new control credential. A rotation that fails or touches anything other than exactly one row, a failed commit, or a crash before commit (reconciled in `ControlService::open`) restores both stores, because the worker never received its new credential. A worker heartbeating with its previous credential during the grace period gets `refresh_due`, so a lost reply self-heals. Fault injection at each boundary: `crates/daemon/tests/control.rs` (rotation failure, crash then restart, lost reply); the restart test fails if startup reconciliation is removed. | Must remain closed | Residual: if the *worker* fails to write its link file after a successful reply and restarts after the 60 s grace, it must re-pair. Keep the worker writing only after success. |
| F6 | Medium | Closed | `CommandTracker::finish_on_shell_exit` finishes the current Command with the child exit code before the Session reaches its terminal state. Tracker and stored-Session tests cover an unterminated training command and a shell exiting at an idle prompt. | Must remain closed | Keep the command update, finish Event, Session transition, and persistence ordered under injected-time tests. |
| F7 | Medium | Open | `Envelope::decode` still requires exact `PROTOCOL_VERSION == 1`; link HTTP payloads have no negotiated version. | Before public compatibility promise | Negotiate major/minimum version and capabilities, with mixed control/worker tests and explicit link API versioning. |
| F8 | Medium | Open | Rust wire IDs remain `u64`; generated TypeScript remains `number` (`TS_RS_LARGE_INT=number`). | Before durable/public protocol promise | Serialize IDs as decimal strings or reject allocation before JavaScript's safe-integer limit; migrate fixtures deliberately. |
| F9 | Medium | Closed | Each Session's saved log stops at 1 GiB with one in-log marker (`session_manager/log_budget.rs`); live output, screen state, and Command tracking continue. `storage.rs` deletes finished Sessions' logs oldest first past an age (default 90 days) or a total budget (default 4 GiB), at start and daily, never a live Session's log; both are `[storage]` settings in `daemon.toml` and zero is rejected. Every JSONL audit file (`audit`, `security-audit`, `link-audit`, `pairing-audit`, `package-audit`) rotates at 16 MiB keeping 3 generations (`private_fs::append_bounded`, 0600, refuses non-regular paths). `sentinal doctor` prints a `Storage:` line with use against the limits. Purge is by setting a tighter limit and restarting; there is no separate delete command. Tests: `log_budget`, `storage`, `private_fs` unit tests, `tests/config.rs`, `crates/cli/tests/doctor.rs`. | Must remain closed | Residuals: on a full disk a log write fails and is logged per chunk while the Session keeps running; remote input is written to the PTY before its audit record, so an audit write failure reports an error for input that was delivered. SQLite rows (Events, Commands) are not yet pruned with their logs. |
| F10 | Medium | Closed | `auth/password.rs` keeps the per-client budget (5 failures, 5 min lock) and adds a global one: 20 failed logins from *any* client key within 15 min lock every client for 5 min and drop the per-key map. Rotating the loopback `Tailscale-User-Login` header buys no extra guesses. `crates/daemon/tests/auth_audit.rs::rotating_client_keys_hit_a_global_failure_budget` failed before the fix. | Must remain closed | Keep both budgets. A local process can now lock the owner out for 5 minutes, an accepted trade against password guessing. |
| F11 | Medium | Closed | `crates/daemon/src/control/proxy.rs::prove_identity` authenticates `/api/v1/link/identity` and pins the returned immutable Machine ID before both WebSocket and output proxying. 4.A adapter/E2E tests cover credential and frame isolation. | Must remain closed | Keep identity proof mandatory on every routed transport and preserve the two-worker isolation tests. |
| F12 | Medium | Partial | The `gpu_smi` fallback gives static Linux builds NVIDIA inventory/metrics. `crates/watcher/src/generic.rs` still treats missing GPU as zero and has no sleep/wake reset; Apple GPU remains unavailable. | Platform claim gate | Reset quiet clocks across sleep/wake, require active work before stall events, represent unavailable GPU distinctly, and document/test macOS behavior. |
| F13 | Medium | Open | Nonce-tagged OSC and bash/zsh/fish hooks remain sound, but there is no explicit compatibility matrix for Powerlevel10k, heredocs, nested shells, SSH/tmux, or VS Code OSC 633. | Platform claim gate | Add real-shell ignored integration cases and document intentionally opaque nesting. |
| F14 | Medium | Partial | `adopt_command::candidates` parses portable `ps etime`, ranks newest first, then truncates. A regression with 45 old high-PID processes and one new PID 42 failed before the fix and passes now; the real adoption integration also passes on macOS. The aggregate ignored command now gets past adoption and stops only at the intentionally unconfigured real-Tailnet enrollment test. | Release/platform evidence | Run the host-local ignored tests as an explicit suite on Linux; keep real-Tailnet/manual benchmarks separate so missing external credentials do not hide local integration results. |
| F15 | Medium | Open | `every_event_kind_has_a_fixture` still compares fixtures with a manually maintained string array, so a new enum variant can evade it. | Quality gate | Derive coverage from an authoritative `EventKind` variant list/schema or an exhaustive compile-time match; repeat the mutation. |
| F16 | Medium | Partial | `eval/` has deterministic generation and scoring, a rules baseline, an OpenAI-compatible runner (tool-call and compact formats; raw replies recorded), and a fine-tuning script. On 2026-10-04: rules F1 0.317 (test); prompted Qwen3-4B F1 0.561 on dev with 2 secret leaks (0 with post-hoc code guards); prompted 0.6B/1.7B below the rules; the 270M–360M runs were invalid (wrong vLLM tool parser, tools dropped by the chat template). A fully fine-tuned Qwen3-0.6B (42.6 min on one RTX 5060 Ti) scored F1 0.978 on the frozen test split, 0 leaks, 99.7% clean negatives, p50 293 ms — but 0.909 on held-out formats, 0.914 on the 8 real logs, and it misread `5e-05` as `0.0005` on a real log. Details: `docs/progress/v2-translator-eval.md`. | V2 only | Hand-label 100–200 real captured screens as the decision metric; add constrained decoding, grounding, secret/URL cleaning, and numeric sanity guards; broaden the generator and retrain; report confidence intervals before any product use. |
| F17 | Low | Open | Security-sensitive files still exceed the approximately 400-line rule: `control/mod.rs` 893, `registry.rs` 672, `protocol/link.rs` 619, `link/mod.rs` 597, and `connect_command.rs` 485 lines. | Post-V1 maintainability | Split by responsibility without behavior changes after the release blockers are closed. |
| F18 | Low | Open | Heartbeat skew uses signed subtraction before `abs`; `ControlClient` silently falls back after secure client-builder failure; credential renames fsync files but not parent directories. | Hardening before 1.0 | Use overflow-safe difference, return client construction errors, fsync parent directories, and fault-test exceptional paths. |
| F19 | Medium | Closed | Found 2026-10-04 during a real TRL fine-tune: the training adapter accepted any `N/M` as step progress and kept the first loss forever, so a script's printed sample log (`step=8500/10000 loss=0.5513`) and Hugging Face loading/tokenizing bars (`4040/4040`, `286/286`) became the run's progress and "loss from". `training.rs` now accepts a bare fraction only in a tqdm bar whose label is empty or about training (colour codes ignored; dataset/tokenize/load/map/eval/download labels refused), plus Keras `N/M [===>` and `iter N/M`; a changed total for the same unit starts a new run and resets loss, first loss, best validation loss, learning rate, and trend. `crates/watcher/tests/training_noise.rs` replays a reconstruction of the real log; rules-baseline progress false positives fell 63 → 45 with equal true positives. | Must remain closed | A deterministic reader still cannot tell a printed `step=…` sample line from real output before the real run starts; the series reset bounds how long that lasts. |
| F20 | Medium | Closed | Found 2026-10-04: Session CPU is a per-core sum (538% for a multi-process tokenizer on 12 cores) but alerts and Health treated it as a Machine percentage, so normal multi-core work produced "resources CRITICAL" and `RESOURCE_HIGH` (now a phone notification). `metrics::machine_cpu_share` divides by logical cores (clamped 0–100) for the alert debouncer and `MetricsObservation::cpu_machine_percent`; stall detection keeps the raw per-core value so a lightly active process is not called idle. Tests in `crates/watcher/tests/health.rs` and `crates/daemon/src/metrics.rs`. | Must remain closed | Keep pressure on the Machine share and activity on per-core CPU; displays may keep `top`'s per-core convention. |

## New finding from the recheck

| ID | Severity | Area | Finding and evidence | Impact | Closure condition |
|---|---|---|---|---|---|
| N1 | High | Delivery — Closed | The reproducible worker release includes `web/dist`; the installer atomically activates/rolls it back, and the existing loopback package listener serves the bounded app shell through Tailnet HTTPS with API precedence and explicit cache/security policy. | Automated production-build acceptance passes; the real-phone run remains evidence, not implementation. | Keep package/service-worker/production Playwright tests green and record the real 4.C phone matrix. |

## Documentation and plan contradictions

| ID | Status | Contradiction | Required correction |
|---|---|---|---|
| C1 | Closed | `docs/report-additions/SENTINEL-PLAN-updated.txt` now points to the canonical `docs/PLAN.md` instead of creating a second plan. | Keep `docs/PLAN.md` canonical. |
| C2 | Closed | `docs/PLAN.md` now marks all capture stages and adopted-log ingestion complete. | Keep plan and Roadmap status synchronized. |
| C3 | Closed | `docs/SETUP.md` now describes the implemented watcher, structured training data, aggregation, and routing. | Update the boundary section with each release. |
| C4 | Closed | `docs/progress/milestone-4.0.md` now records the separate credential-file/SQLite write boundary and points at the unresolved recovery gate. | Close F5 before claiming crash-atomic refresh. |
| C5 | Closed | `docs/SECURITY.md` now lists public, bootstrap, terminal/data, owner, exchange, and link-auth route classes. | Keep the list aligned with router tests. |
| C6 | Closed | `docs/SECURITY.md` now documents direct-IP and Serve-header keys plus the residual spoofing risk tracked by F10. | Close F10 before removing the limitation. |
| C7 | Closed | `just acceptance` runs both `just check` and Playwright; `docs/TESTING.md` defines when the combined gate is required. | Use the combined target for routing/mobile release candidates. |
| C8 | Closed | The old Milestone 1 prompt is explicitly labelled historical and names the current service files. | Retain the old names only as compatibility context. |
| C9 | Closed | `docs/progress/README.md` defines progress reports as point-in-time evidence and this tracker's verification table as the current count. | New reports must record their tested commit. |
| C10 | Partial | Endpoint identity verification is mandatory (F11 closed), and Recipe context validation closes F2. Credential crash recovery and the `just check`/Playwright distinction remain likely agent failure points. | Close F5 and C7, then remove this row. |

## Verification record

| Check | 2026-10-04 result |
|---|---|
| `just acceptance` (2026-10-05, Milestone 4.C candidate) | Pass: complete `just check`, 31 web files / 144 tests, 20 evaluator tests, all shell checks, then 5 production-build Playwright scenarios including two fake workers and local fake-ntfy delivery. |
| `just acceptance` | Pass after the repository/documentation sync: the complete `just check` gate followed by all 5 Playwright scenarios. |
| `just check` | Pass: format, all-feature Clippy with warnings denied, workspace tests, TS binding check, 30 web files / 140 tests, 8 evaluator tests, and all shell checks. |
| `just test-e2e` | Pass: 5 Playwright tests, including two workers and the 390×844 mobile Recipe flow. |
| `cargo doc --workspace --no-deps` | Pass without warnings after fixing the stale `<tailnet>` rustdoc markup. |
| Adoption mutation/regression | The new low-PID process test failed against PID sorting and passes after elapsed-time sorting. |
| Real adoption integration | Pass on macOS outside the filesystem sandbox. |
| Full `--ignored` aggregate | Not fully run: adoption and auto-wrap passed; execution stopped at the real-Tailnet enrollment test because `SENTINAL_ENROLL_CONTROL_URL` was intentionally not supplied. This is an external acceptance prerequisite, not a product test failure. |
| Deterministic translator baseline | 1,528 cases: F1 0.317, precision 0.661, recall 0.209, exact match 22.8%, zero secret-leak examples, clean negatives 95.9%. |
| Qwen3-4B exploratory model run | 286 dev cases, no request/parse failures, final prompt: F1 0.561, precision 0.606, recall 0.523, exact match 40.6%, two secret-leak examples (Jupyter token copied into a URL), clean negatives 70.9%, p50 1,454 ms, p95 4,910 ms. Fails the safety gate on its own; post-hoc URL stripping and grounding reach 0 leaks and F1 0.568. |
| Fine-tuned Qwen3-0.6B translator | Frozen test split (1,528) run once: F1 0.978, exact match 96.0%, zero secret leaks, clean negatives 99.7%, invented values 0.9%, p50 293 ms / p95 894 ms; held-out formats 0.909; real logs 0.914. Synthetic-data caveats in `docs/progress/v2-translator-eval.md`. |

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
- [x] F3 browser/WS bearer exposure is removed.
- [x] F5 credential refresh is crash-recoverable and idempotent.
- [x] F6 a shell exit cannot leave a RUNNING Command.
- [x] F9 storage has enforceable limits and purge behavior.
- [x] F10 login has a non-spoofable/global secondary throttle.
- [ ] F14 all ignored integration tests pass on the supported macOS and Linux hosts.
- [x] N1 the production PWA is packaged behind the control origin.
- [ ] The exact 4.A and 4.B two-real-Machine acceptance runs are appended to
      their progress reports, including OS versions and tested commit.
- [ ] The release notes state the F1 daemon-restart limit, or R1 is complete.

## Next remediation order

1. Make credential refresh recoverable and idempotent (F5).
2. Add storage quotas, rotation, usage, and purge (F9).
3. Add the global/non-spoofable login throttle (F10).
4. Run the host-local ignored suite on Linux and keep credential-dependent
   Tailnet acceptance separate (F14).
5. Run and record the real 4.A/4.B/4.C acceptance on the release candidate.
6. Address protocol evolution and ID representation (F7/F8) before calling the
   wire format stable.
7. Reconcile the documentation contradictions C1-C9 with the implementation.
8. Schedule the R1 keeper decision from actual V1 use evidence, while keeping
    the daemon-restart limitation explicit.
