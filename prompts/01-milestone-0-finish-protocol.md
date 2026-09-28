# MILESTONE 0 (finish) — Process, Metric, TypeScript types

Goal: complete the contract so later milestones never need to invent types.

## Tasks

1. **`Process`** in `crates/protocol/src/process.rs`:
   `pid: u32`, `parent_pid: Option<u32>`, `name`, `cmdline: Vec<String>`,
   `state: ProcessState` (`RUNNING | SLEEPING | STOPPED | ZOMBIE | UNKNOWN`),
   `cpu_percent: f32`, `rss_bytes: u64`, `gpu_mem_bytes: Option<u64>`,
   `started_at: Timestamp`.
   Helper: `fn tree_totals(processes: &[Process], root: u32) -> ProcessTotals`
   summing CPU/RAM/VRAM for `root` and all descendants. Must not loop forever
   on a cycle in parent links.
2. **`Metric`** in `crates/protocol/src/metric.rs`:
   `MachineMetrics { at, cpu_percent, ram_used_bytes, ram_total_bytes, gpus: Vec<GpuMetrics> }`,
   `GpuMetrics { index, util_percent, vram_used_bytes, vram_total_bytes, temp_c: Option<f32> }`,
   `SessionMetrics { session_id, at, totals: ProcessTotals }`.
   Helpers: `ram_percent()`, `vram_percent()`; both return `None` when total is 0.
3. Add to `DaemonMessage`: `MACHINE_METRICS { metrics }`, `SESSION_METRICS { metrics }`.
   Add to `ClientRequest`: `GET_HISTORY { session_id: Option<SessionId>, limit: u32 }`.
   This is additive: keep `PROTOCOL_VERSION = 1`.
4. **TypeScript generation:** add `ts-rs` behind a `ts` feature; every wire type
   derives `TS`. `just gen-ts` writes `web/src/protocol/*.ts` (create the folder
   only). A test fails if generated files are out of date (`just gen-ts --check`
   or a test comparing output to committed files).

## Tests first

- Unit: `tree_totals` with a root and nested children; with an unknown root
  (all zeros); with a parent-link cycle (terminates, counts each pid once).
- Unit: percent helpers, including total 0.
- Contract: fixtures `process.json`, `machine_metrics.json`, `session_metrics.json`;
  extend `daemon_messages.json` and `client_requests.json`; extend the
  "every kind has a fixture" tests.

## Done when

`just check` passes, every new wire type has a fixture, TS files are
generated and checked, and ROADMAP Milestone 0 is fully ticked. Then send
the report and stop.
