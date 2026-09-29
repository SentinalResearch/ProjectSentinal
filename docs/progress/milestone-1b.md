# Milestone 1b progress — Any terminal into Sentinal

Status: Milestone 1b.C complete 2026-09-28; awaiting human review before Milestone 3.

## Pre-Milestone 3 audit remediation

- [x] Add a one-command Tailscale-only daemon/web launcher —
  `scripts/tests/run-network-test.sh` plus a manual health/login smoke test
- [x] Deliver reduced-capture Metrics, Commands, Events, and state through the real web adapter — daemon behaviour tests and `web/src/data/api/index.test.ts`
- [x] Make AUTO_WRAP attachment a bounded, confirmed handoff with safe orphan cleanup — CLI and daemon auto-wrap regression tests
- [x] Preserve external-shell Command order when background reporters arrive out of order — `crates/daemon/tests/external_shell.rs`
- [x] Close the screen-snapshot/live-output subscription gap — `crates/daemon/tests/session_manager.rs`
- [x] Enforce explicit adopted-log ownership and safe local-socket ancestors — adopt and local-socket filesystem tests
- [x] Give Playwright a fresh persistent data directory on every run — repeated `pnpm -C web test:e2e`
- [x] Re-run `just check`, every ignored integration available on this host, and Playwright before starting Milestone 3

## Pre-Milestone 3 audit verification (2026-09-28, macOS arm64)

- `just check` passes: fmt, clippy `-D warnings`, 348 fast Rust tests,
  45 TypeScript export checks, and 48 web tests.
- All 14 ignored integration/benchmark tests pass on this host. Real zsh and
  bash paths ran; fish-specific scripts reported their documented skip
  because fish is not installed. The hook benchmark remained below budget:
  added p95 1.13 ms zsh and 2.48 ms bash.
- Playwright passed twice consecutively (2/2 each run) with a newly generated
  private daemon config, authentication file, token, SQLite data directory,
  and local socket per run.
- No protocol fixture or protocol-version change and no new dependency.
- Remaining Milestone 1b release work is unchanged: 1b.D log tail/UI lands
  with Milestone 3; Linux and real-fish platform rows remain pending.

## Tailscale launcher verification (2026-09-29, macOS arm64)

- `scripts/tests/run-network-test.sh` passes with generated-config/proxy
  assertions and rejection of wildcard, LAN, out-of-range CGNAT, and invalid
  ports. It is part of `just check`.
- `just network` discovered `100.93.101.56`, built and started `sentinald` on
  port 7420, and started Vite on port 5173. Direct daemon health and proxied
  frontend health both returned `{"ok":true,"version":1}`; the frontend root
  returned the Vite HTML page.
- Ctrl-C stopped both listeners and removed the ephemeral configuration.
  Persistent Sessions, token, and owner authentication remain in the normal
  per-user directories. No dependency or protocol change was required.
- Final `just check` passes: formatting, clippy with warnings denied, all
  Rust tests, generated TypeScript, 48 web tests, and the launcher test.

## Plan

- [x] 1b.0 contract and capability helpers — `crates/protocol/src/session.rs`,
  `crates/protocol/src/message.rs`, protocol unit/contract fixtures, legacy decode,
  and generated TypeScript checks
- [x] 1b.0 private local ingress and peer UID verification —
  `crates/daemon/tests/local_socket.rs` plus `local_socket_integration`
- [x] 1b.0 capture enforcement — SessionManager and in-process API behavior tests
- [x] 1b.0 legacy persistence/privacy — `crates/store/tests/store.rs` and audit assertions
- [x] Sentinal Watch and no-nesting behavior — CLI unit tests, fake daemon behavior tests, real PTY integration
- [x] External hook generation/install/uninstall — script tests against a fake socket and byte-exact rc fixtures
- [x] Hook performance/fail-open behavior — injected transport tests plus recorded Linux/macOS benchmark
- [x] Auto-wrap guards and 300 ms fallback — table-driven unit tests and manual IDE/SSH smoke matrix (manual IDE/SSH matrix not yet run)
- [x] Read-only adoption and unknown exit — fake process table, same-user tests, Linux/macOS integration (Linux pending)
- [ ] Rotation-safe log tail — temp-file append/truncate/rotate/partial-line behavior tests
- [ ] Capture-aware web UI — Testing Library coverage for badges and forbidden actions (1b.C: detail/split badge, unknown exit, read-only terminal area, log hint; completion in 1b.D)
- [ ] Privacy review — inspect SQLite, audit JSONL, PTY logs, and adopted-log metadata
- [ ] Full verification — `just check`, Playwright, ignored integration suite, platform matrix

## 1b.0 verification (2026-09-28, macOS)

- `just check`: fmt, clippy `-D warnings`, `cargo test --workspace`,
  `just gen-ts --check`, and `pnpm -C web check` all pass.
- `cargo test -p daemon --test local_socket -- --ignored`
  (`local_socket_integration`) passes on macOS with real peer credentials;
  Linux run still pending.
- Live smoke with `TA_FAKE_PTY=1 tad`: parent `drwx------`, socket
  `srw-------`, `ADOPT_PROCESS pid 0` → `INVALID_REQUEST`, valid request →
  `UNAVAILABLE`, SIGTERM exits 0 and removes the socket. An over-long
  socket path fails startup with `PathTooLong`.
- Web: only generated types and the `CREATE_SESSION` payload changed
  (`origin: "UI"`); capture-aware UI remains in 1b.D.

## 1b.C verification (2026-09-28, macOS arm64, zsh 5.9 / bash 3.2.57)

- `just check` passes: 385 Rust tests (14 ignored integrations/benchmark)
  and 47 web tests. Playwright 2/2.
- Ignored, all 14 pass here, including new `auto_wrap_integration` (real
  zsh and bash enter exactly one AUTO_WRAP / FULL Session, no nesting,
  `exit` closes the terminal with status 0; with a hung daemon the original
  shell starts with identical `$0`/cwd/env marker and `exit 3` status,
  added start time +231 ms zsh / +214 ms bash) and
  `adopt_process_integration` (real same-user child adopted read-only,
  `--pick` cancel adopts nothing, duplicate refused, real `lsof` discovers
  the redirected log as STDOUT_AND_STDERR, live SESSION_METRICS arrive
  without attaching, remote kill refused with the process alive, and on
  exit the Session is COMPLETED with unknown exit and no Command).
- Found and fixed along the way: attach/watch clients hung after the shell
  exited (Tokio stdin kept the runtime alive; the exit monitor also never
  notified attached clients); reduced-capture Metrics never reached the web
  (they needed an attachment); clients could mistake the new observation
  messages for replies (CLI and web).
- Interpretation: "PID reuse" is handled by never letting a reused PID
  inherit a Session (the stale Session ends; the new process can be adopted
  separately). ADOPT_PROCESS has no start-time field to reject a PID that
  was reused between listing and adopting.
- Platform-specific discovery gaps: Linux `/proc/<pid>/fd` discovery is
  covered only by sample-data tests (no Linux host); macOS `lsof` does not
  flag deleted files, so they are excluded only because the path no longer
  resolves to a regular file; no discovery on other Unix systems; sysinfo
  may report no cwd/executable for some macOS processes (recorded as empty,
  not an error); fish auto-wrap (needs fish ≥ 3.2 for `status fish-path`)
  and fish adoption flows are unexecuted here.
- Not run: manual IDE / SSH smoke matrix for auto-wrap, and all Linux rows.

## 1b.B verification (2026-09-28, macOS arm64, zsh 5.9 / bash 3.2.57)

- `just check` passes: 334 Rust tests (12 ignored integrations/benchmark)
  and 41 web tests. New fast tests: 16 daemon external-shell, 1 store
  restart, 2 process-probe, 2 socket-path, 11 rc fixtures, 12 installer /
  script, 12 reporter (paused-clock 50 ms deadline, silent binary,
  suppression).
- Ignored, run here: `hook_script_integration` (real zsh and bash vs a fake
  socket: register/start/finish, exact text incl. metacharacters, cwd after
  `cd`, exit codes, `$?` preserved, no output in socket traffic, zero messages
  with `SENTINAL_SESSION_ID` or `SENTINAL_DISABLE=1`, silent prompt when the
  socket is missing, hangs, or replies garbage) and
  `external_shell_hook_integration` (real `sentinald`: Commands stored with
  exact text/exit/cwd, attach refused, Session COMPLETED with unknown exit
  when the shell exits, no output in SQLite/audit/logs). Both pass; all other
  ignored integrations and Playwright (2) still pass.
- Benchmark (`hook_benchmark`, release, 50 warmup + 500 iterations): added
  p95 zsh 1.47 ms, bash 2.55 ms — below the 5 ms gate. Table in
  `docs/SHELL_HOOKS.md`.
- By hand: install / repeated install (no change) / uninstall (byte-exact,
  mode 0640 kept, 0600 backups) / malformed marker and unknown shell refused;
  an interactive zsh with the block installed and no daemon printed nothing
  and kept `$?`.
- Missing platform rows: fish (not installed on this host; hook written to
  fish 3.1+ syntax but unexecuted), all Linux shells, and bash 5.x.

## 1b.A verification (2026-09-28, macOS arm64, zsh 5.9 / bash 3.2)

- `just check` passes: 278 Rust tests (9 ignored integrations) and 41 web
  tests. New: 7 protocol rendering unit tests, 2 quoting contract tests,
  6 daemon Watch tests, 14 CLI Watch tests, 1 Playwright test.
- `cargo test --workspace -- --ignored` passes all 9, including
  `watch_integration` (real `sentinald`, `sentinal watch` in a PTY, client
  killed, same shell PID reattached from a second PTY, Command stored once)
  and `shell_quoting_integration` (bash and zsh; fish not installed here).
- `pnpm -C web test:e2e`: 2 passed, including a WATCH/FULL Session that
  appears by title and attaches.
- Manual demo on a real daemon: `sentinal watch -t "Web demo job" -- sh -c
  '…ticks…'`, detach (exit 0 with reattach hint), then the web UI showed the
  Session RUNNING and attached with ticks still advancing.
- Found and fixed: a 0x0 terminal size (some PTYs report it) made Watch and
  attach fail; it now means "unknown" and falls back to 80x24.
- Not yet covered: Linux run of the ignored suite; fish on a host with fish.
- Observed, out of scope: when `sentinald` stops, Session processes are not
  terminated (existing behavior); under a parent that ignores SIGHUP they
  keep running unobserved. Worth deciding before auto-wrap (1b.C).

## Proposed implementation sequence

1. 1b.0 protocol/capabilities and local socket.
2. 1b.A Watch, including product binary rename compatibility.
3. 1b.B ordinary-shell hook install/reporting.
4. 1b.C auto-wrap and read-only process adoption.
5. Milestone 3 watcher engine.
6. 1b.D adopted log tail and final capture-aware UI acceptance.

Each numbered stage ends with a commit and human review. Do not start the
next stage automatically.

## Execution prompts

- `prompts/03a-milestone-1b0-contract-local-ingress.md`
- `prompts/03b-milestone-1ba-sentinal-watch.md`
- `prompts/03c-milestone-1bb-external-shell-hook.md`
- `prompts/03d-milestone-1bc-auto-wrap-adopt.md`
- `prompts/04-milestone-3-events-watchers.md`
- `prompts/04a-milestone-1bd-adopted-log-ui.md`

## Approved 1b.C design gate (2026-09-28)

- Session gains `log: Option<SessionLog>` (serde default, PROTOCOL_VERSION
  stays 1) with `SessionLog { path, source: EXPLICIT | STDOUT | STDERR |
  STDOUT_AND_STDERR }`. Only ADOPTED Sessions carry it; capture stays PROCESS
  until 1b.D tails the log.

## Approved 1b.0 design gates

- Watch's initial Command is structured `Vec<String>` argv. Shell text may
  only be rendered by the protocol quoting helper.
- `Session.pid` is always the process-tree root, paired with the OS-observed
  `process_started_at` when known. PID zero is invalid.
- External process exit with no status is `COMPLETED` with no `exit_code` and
  is displayed as “Ended · exit code unknown”; no new version-1 state is added.
- Privileged local reporting uses a separate `LocalRequest` contract and can
  never be decoded or dispatched by the remote WebSocket API.
- Local messages use a 4-byte big-endian length followed by at most 65,536
  bytes of versioned JSON.
- Linux prefers `$XDG_RUNTIME_DIR/sentinal/sentinald.sock`; macOS and the
  validated Linux fallback use `<data_dir>/run/sentinald.sock`.
- `ta`/`tad` compatibility aliases remain through the 1b migration; the
  installed Sentinal names land in 1b.A.
