# RELIABILITY R1 — FULL Sessions survive `sentinald` restart

Goal: a daemon crash, service restart, or binary upgrade does not terminate a
daemon-owned FULL Session. After `sentinald` returns, the owner can attach to
the same process, reconstructed screen, continuous log, and Session ID.

This is not ordinary detach persistence. Today `sentinald` owns every PTY, so
its death closes the PTY and restart recovery correctly marks the Session
`INTERRUPTED`. Achieving this goal requires a deliberate ownership change.

Depends on a stable V1 and Milestone 1c packaging. Do not implement this “while
there” in another milestone.

## Required architecture review — stop before tests or implementation

Present and compare at least:

1. A small per-Session keeper process owned by the same user (recommended
   direction), with `sentinald` as controller.
2. One shared local supervisor process.
3. An external multiplexer such as tmux/screen (expected to be rejected unless
   the human explicitly approves the new runtime dependency and contract).

For the recommended proposal define:

- which process owns the PTY child, master, raw log writer, VT state, OSC
  parsing, scrollback, process exit wait, and input audit responsibility;
- binaries/process tree and why this is not a second network-exposed daemon;
- private IPC location, framing, authentication/capability secret, peer UID,
  modes, maximum messages, backpressure, and stale endpoint cleanup;
- keeper discovery/reclamation after daemon restart, PID-reuse defense, and
  binding to persisted Session ID + process start identity;
- what happens during intentional service stop, upgrade, OS logout, reboot,
  keeper crash, daemon/keeper version mismatch, and simultaneous daemon start;
- how output is buffered while the daemon is absent, bounded disk/memory
  behavior, log continuity, and VT/tracker reconstruction;
- how Command/Event/Metric persistence catches up without duplicate Commands
  or Events;
- who authorizes and audit-logs terminal input while the daemon is connected;
- how a normal Session kill reaches the keeper and cannot target a foreign
  process;
- compatibility/migration for existing Sessions and protocol version impact;
- service-manager changes and upgrade/rollback ordering on macOS and Linux.

Show process/sequence diagrams for create, normal attach, daemon crash,
reclaim, keeper crash, kill, and upgrade. Provide concrete persisted records
and IPC examples. Identify any dependency or `unsafe` requirement. Then stop
and request explicit human approval.

Do not change fixtures, production code, services, or process ownership before
approval. If the design needs a dependency outside the fixed stack, stop under
the dependency rule.

## Implementation constraints after approval

- Keep all network authentication and authorization in `sentinald`; keepers
  listen only on private same-user local IPC and accept no browser/remote
  protocol.
- No keeper may attach to, signal, or claim a process it did not create.
- Keeper credentials are random, mode `0600`, narrowly scoped to one Session,
  compared without early exit, rotated on safe reclamation if approved, and
  never logged.
- Reclaim validates UID, executable identity, Session identity, start time,
  protocol version, and persisted capability before accepting state.
- Missing/inconsistent/foreign keepers fail closed and produce a structured
  interruption event. Never guess that a process is still the original one.
- Output remains untrusted and is never executed during replay/reconstruction.
- Preserve existing browser/CLI wire compatibility unless the approved design
  explicitly requires an additive change.

## Tests first

- Pure state-machine tests for create/claim/reclaim/version mismatch, duplicate
  claimant, PID reuse, missing keeper, keeper crash, intentional stop policy,
  and upgrade rollback with injected identities/clocks.
- Fake IPC tests for framing, limits, peer UID, secret comparison, replay,
  backpressure, disconnect/reconnect, stale endpoint, and payload privacy.
- Fake PTY: daemon disappears during output and Command execution, keeper
  retains bounded data, restarted daemon rebuilds exact screen/Command state,
  and subsequent input reaches the same fake child once.
- Storage: no duplicate Command/Event rows; continuous log is decompressible;
  Session ID, root PID identity, state, and audit metadata remain coherent.
- Security: foreign daemon/keeper/endpoint and replaced credential are refused
  before payload processing; terminal bytes never enter security logs.
- Integration `daemon_restart_session_integration` (`#[ignore]`) on Linux and
  macOS: start a long process, hard-kill `sentinald`, observe child survival,
  restart, reattach, verify same PID/screen, input, graceful exit, and logs.
- Upgrade integration with old/new compatible keeper versions as defined by
  the approved matrix.

No sleeps over 100 ms in fast tests; coordinate with channels and injected
clocks. Real process/service tests remain ignored and explicit.

## Done when

A real FULL Session survives daemon crash and upgrade on the available host,
reattaches with the same verified process and screen, security failure modes
are tested, services/install docs are updated, `just check` passes, Roadmap R1
is checked, and the report states Linux/macOS coverage plus reboot/logout
limitations. Commit and stop.
