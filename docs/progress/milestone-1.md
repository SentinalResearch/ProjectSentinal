# Milestone 1 plan

## Tasks

- [ ] Create the daemon and CLI crates plus the minimal web project; prove the
  workspace remains buildable with crate and web checks.
- [ ] Implement config loading, safe bind validation, token creation, constant-time
  authentication, and audit records; prove them in daemon unit tests and
  `crates/daemon/tests/api.rs`.
- [ ] Implement `PtyBackend`, real and fake PTYs, `SessionManager`, screen restore,
  bounded scrollback, compressed logs, attach/detach, resize, input, exit, and
  kill; prove them in `crates/daemon/tests/session_manager.rs`.
- [ ] Implement the Axum health, machine, sessions, and WebSocket API with binary
  framing; prove request validation and connection recovery in
  `crates/daemon/tests/api.rs`.
- [ ] Implement `ta ls`, `new`, `attach`, and `kill` against the WebSocket API;
  prove argument and frame handling with CLI unit tests.
- [ ] Implement the React/xterm session list and terminal, resize handling, token
  entry, and mobile key bar; prove framing and keys with Vitest and the main flow
  with one Playwright smoke test.
- [ ] Add systemd and launchd user service files, document the manual demo, run all
  Rust/web/integration checks, update the roadmap, and commit the milestone.

## Open questions

- None. The three additional Rust dependencies needed for the CLI and in-process
  API tests were approved by the owner before implementation.

## Verification

- Pending.
