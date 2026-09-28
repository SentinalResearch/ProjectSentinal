# MILESTONE 1b.0 — Capture contract and private local ingress

Goal: establish the security and protocol foundation for bringing ordinary
terminals and existing processes into Sentinel. This stage does **not** add
`sentinel watch`, edit shell rc files, auto-wrap shells, or adopt a process.

Read `docs/PLAN.md`, `docs/CAPTURE_MODES.md`, `docs/SECURITY.md`,
`docs/CLI.md`, `docs/SHELL_HOOKS.md`, and
`docs/progress/milestone-1b.md` before changing code.

## Required design review — stop before tests or implementation

Inspect the current protocol and present one concrete additive wire proposal
covering all of these points:

1. `SessionOrigin`: `UI | WATCH | SHELL_HOOK | AUTO_WRAP | ADOPTED`.
2. `CaptureLevel`: `FULL | COMMANDS | PROCESS_LOG | PROCESS`.
3. Session process identity. `pid` should have one documented meaning:
   process-tree root for every origin, or `None` when no root is known.
4. `CREATE_SESSION` additions for `origin`, `title`, and an optional initial
   Command. Prefer structured `Vec<String>` argv; do not define executable
   text without explicit shell/quoting semantics.
5. Exact payloads for `REGISTER_SHELL`, `SHELL_COMMAND_STARTED`,
   `SHELL_COMMAND_FINISHED`, and `ADOPT_PROCESS`.
6. How external Session termination and adopted “exit code unknown” map to
   structured state without claiming success or breaking old clients.
7. Whether local-only messages are a separate `LocalRequest` enum or guarded
   `ClientRequest` variants. Remote HTTP/WebSocket clients must not gain local
   registration or adoption authority.
8. Local framing, maximum message size, error replies, and compatibility.
9. Socket location on Linux and macOS, including fallback behavior.

Show representative JSON for every new request and the updated Session. Show
how old version-1 Session/CreateSession JSON remains readable. Then stop and
ask the human to approve the proposal. Do not edit fixtures or production
code until approval is explicit.

If the approved design is additive, keep `PROTOCOL_VERSION = 1`. If it cannot
be made backward compatible, report the fixture break and stop.

## 1. Protocol and capabilities

After approval:

- Add the approved wire types only in `crates/protocol` and derive generated
  TypeScript through `ts-rs`.
- Update fixtures by hand and extend every-kind coverage.
- Preserve old Session/CreateSession fixture decoding with `serde` defaults
  where the approved design permits it.
- Add plain capability helpers used by every interface. At minimum capture
  must answer whether a Session can attach/read screen, accept input, resize,
  be killed as a daemon-owned PTY, expose Commands, expose output/logs, and
  expose Metrics.
- Capability is derived from capture, never from UI assumptions or a claimed
  origin.
- Validate PID `0`, empty Command argv, empty Command executable, illegal
  origin/capture combinations, empty cwd, oversized text, and invalid local
  request fields with errors that name the bad input.
- Prove previously stored Session JSON still loads and receives safe defaults.

## 2. Private local Unix socket

- Add a daemon-owned per-user Unix listener using the approved location.
- Create its parent directory owner-only and the socket mode `0600`.
- Check the OS peer UID before decoding or acting on a message. A wrong or
  unavailable UID is rejected and audit-logged without request payload data.
- Refuse unsafe paths, symlinks, or a socket owned by another user. Remove a
  stale socket only after ownership/type checks.
- Enforce the approved frame/message limit before allocating an unbounded
  buffer.
- Bad JSON, invalid messages, and unsupported protocol versions return a
  bounded structured error without stopping the listener.
- Local reporting requests must never attach, resize, send input, or signal a
  process.
- Shut down cleanly and remove only the socket instance this daemon created.
- Put peer-identity and socket dispatch logic behind small injectable
  functions. Do not add a trait unless there are two actual implementations.
- Use existing dependencies. Stop if safe peer-UID verification would require
  a new dependency or `unsafe`.

## 3. Enforce capture at existing boundaries

- Existing UI-created Sessions become `UI` / `FULL` without changing their
  behavior.
- WebSocket `ATTACH`, terminal binary input, `RESIZE`, and `KILL` consult the
  same daemon capability check.
- Reduced-capture requests return `DaemonMessage::Error` and keep the
  connection alive.
- Local-only request types sent over remote WebSocket are rejected and cannot
  mutate state.
- Audit records remain metadata-only. No request or input payload is written
  to the audit log.

## Tests first

- Protocol unit: capability matrix for every capture level; origin/capture
  defaults; PID `0`; empty/oversized Command data; invalid combinations.
- Contract: fixture for every origin, capture, and approved request; old
  Session/CreateSession fixtures still decode; TypeScript generation is
  current.
- Store: pre-1b serialized Session rows load with safe defaults and can be
  updated without a schema reset.
- Local socket behavior using fakes/injected identity: same UID accepted,
  wrong UID rejected before decode, malformed/oversized messages recover,
  local-only operations cannot send PTY input.
- Filesystem: parent permissions, socket mode `0600`, safe stale cleanup,
  symlink and wrong-owner refusal.
- API: every reduced capture rejects attach/input/resize/kill as applicable;
  remote local-only request returns `Error` and the connection stays open.
- Integration `local_socket_integration` (`#[ignore]`): real Unix socket peer
  credentials and mode on Linux/macOS.
- Privacy: audit and store assertions prove payload bytes are absent.

No network access, real sleeps, or machine-user assumptions in fast tests.

## Done when

The approved additive contract, generated TypeScript, capability enforcement,
and private local listener are complete; `just check` passes; the ignored
local socket integration is run on the available host; Roadmap 1b.0 and the
progress file are updated; exact socket inspection commands are documented in
the report. Commit logical changes, send the required report, and stop before
Milestone 1b.A.
