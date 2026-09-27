# Milestone 0 finish plan

## Tasks

- [x] Add `Process`, `ProcessState`, and cycle-safe `tree_totals`; prove them with
  unit tests in `crates/protocol/src/process.rs` and the `process.json` contract
  fixture.
- [x] Add machine, GPU, and session Metric types plus zero-safe percentage
  helpers; prove them with unit tests in `crates/protocol/src/metric.rs` and the
  `machine_metrics.json` and `session_metrics.json` contract fixtures.
- [x] Extend `DaemonMessage` and `ClientRequest` additively; prove every variant
  with `daemon_messages.json`, `client_requests.json`, and completeness checks in
  `crates/protocol/tests/contract.rs`.
- [x] Derive TypeScript definitions for every wire type behind the `ts` feature,
  generate committed files in `web/src/protocol/`, and make `just gen-ts --check`
  detect stale output.
- [x] Run `just check`, update the Milestone 0 roadmap checkboxes, and record the
  final verification here.

## Open questions

- None. The additions are backward-compatible, so `PROTOCOL_VERSION` remains 1.

## Verification

- `just check` passes: formatting, all-feature clippy, 92 Rust tests, and 30
  TypeScript export/freshness tests.
- `just gen-ts --check` reproduces `web/src/protocol/*.ts` with no diff.
- All 30 serialized Rust wire types derive `TS` when the `ts` feature is enabled.
- Contract additions are additive and `PROTOCOL_VERSION` remains 1.
