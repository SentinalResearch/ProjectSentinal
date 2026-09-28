# Milestone 1b progress — Any terminal into Sentinel

Status: planned; next implementation priority after Milestone 2.

## Plan

- [ ] Contract and capability helpers — protocol unit/contract fixtures and generated TypeScript checks
- [ ] Private local ingress and peer UID verification — daemon unit/behavior tests plus Linux/macOS integration
- [ ] Sentinel Watch and no-nesting behavior — CLI unit tests, fake daemon behavior tests, real PTY integration
- [ ] External hook generation/install/uninstall — script tests against a fake socket and byte-exact rc fixtures
- [ ] Hook performance/fail-open behavior — injected transport tests plus recorded Linux/macOS benchmark
- [ ] Auto-wrap guards and 300 ms fallback — table-driven unit tests and manual IDE/SSH smoke matrix
- [ ] Read-only adoption and unknown exit — fake process table, same-user tests, Linux/macOS integration
- [ ] Rotation-safe log tail — temp-file append/truncate/rotate/partial-line behavior tests
- [ ] Capture-aware web UI — Testing Library coverage for badges and forbidden actions
- [ ] Privacy review — inspect SQLite, audit JSONL, PTY logs, and adopted-log metadata
- [ ] Full verification — `just check`, Playwright, ignored integration suite, platform matrix

## Proposed implementation sequence

1. 1b.0 protocol/capabilities and local socket.
2. 1b.A Watch, including product binary rename compatibility.
3. 1b.B ordinary-shell hook install/reporting.
4. 1b.C auto-wrap and read-only process adoption.
5. Milestone 3 watcher engine.
6. 1b.D adopted log tail and final capture-aware UI acceptance.

Each numbered stage ends with a commit and human review. Do not start the
next stage automatically.

## Open questions / design gates

- Choose the wire representation of Watch's initial Command. Structured argv
  is preferred for exactness and injection resistance; a shell string would
  need explicit shell and quoting semantics.
- Define whether the planned Session PID is the daemon-owned shell root,
  external shell PID, or adopted process root in every origin. Prefer one
  documented “process-tree root” meaning.
- Decide the compatibility window for installed binary names (`ta`/`tad`
  aliases alongside `sentinel`/`sentineld`).
- Define portable external-shell termination when PID reuse is possible.
- Approve the exact per-user Unix socket location on Linux and macOS.

These questions must be resolved before protocol fixtures or on-disk state
are changed.
