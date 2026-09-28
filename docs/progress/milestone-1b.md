# Milestone 1b progress — Any terminal into Sentinal

Status: Milestone 1b.A complete 2026-09-28; awaiting human review before 1b.B.

## Plan

- [x] 1b.0 contract and capability helpers — `crates/protocol/src/session.rs`,
  `crates/protocol/src/message.rs`, protocol unit/contract fixtures, legacy decode,
  and generated TypeScript checks
- [x] 1b.0 private local ingress and peer UID verification —
  `crates/daemon/tests/local_socket.rs` plus `local_socket_integration`
- [x] 1b.0 capture enforcement — SessionManager and in-process API behavior tests
- [x] 1b.0 legacy persistence/privacy — `crates/store/tests/store.rs` and audit assertions
- [x] Sentinal Watch and no-nesting behavior — CLI unit tests, fake daemon behavior tests, real PTY integration
- [ ] External hook generation/install/uninstall — script tests against a fake socket and byte-exact rc fixtures
- [ ] Hook performance/fail-open behavior — injected transport tests plus recorded Linux/macOS benchmark
- [ ] Auto-wrap guards and 300 ms fallback — table-driven unit tests and manual IDE/SSH smoke matrix
- [ ] Read-only adoption and unknown exit — fake process table, same-user tests, Linux/macOS integration
- [ ] Rotation-safe log tail — temp-file append/truncate/rotate/partial-line behavior tests
- [ ] Capture-aware web UI — Testing Library coverage for badges and forbidden actions
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
