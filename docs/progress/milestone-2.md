# Milestone 2 progress — Command tracking, history, metrics

## Plan

- [x] Shell hooks and launch integration — `crates/daemon/tests/shell_hooks_integration.rs`, hook source tests
- [x] Streaming OSC parser — `crates/daemon/src/osc.rs` unit tests
- [x] Deterministic command tracker — `crates/daemon/src/tracker.rs` unit tests
- [x] SQLite migrations and store API — `crates/store/tests/store.rs`
- [x] Restart recovery and metric retention — `crates/store/tests/store.rs`
- [x] Machine/session sampling, optional NVIDIA, and high-resource debounce — metrics unit tests
- [x] Session-manager/store/tracker/metrics integration — daemon behaviour tests
- [x] Additive history/search WebSocket contract and fixtures — protocol contract tests
- [x] Real history/search/resource UI — Vitest adapter and component tests
- [x] Full verification — `just check`, ignored shell integrations where installed, Playwright smoke

## Open questions

- Resolved: add `HISTORY` and `SEARCH_COMMANDS` as backward-compatible protocol variants; keep protocol version 1.
- NVIDIA support is compile-time optional and is not exercised on machines without NVML.

## Verification

- `just check`: passed (format, Clippy with warnings denied, 178 Rust tests, generated TypeScript check, TypeScript compile, 41 Vitest tests).
- `pnpm -C web test:e2e`: 1 Playwright smoke test passed against a real daemon with its fake-backend mode.
- `cargo test -p daemon --tests -- --ignored --nocapture`: real PTY, host process sampling, zsh, and bash passed; fish performed its documented clean skip because fish is not installed on this host.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: passed, including the optional NVIDIA feature.
