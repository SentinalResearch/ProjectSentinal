# MILESTONE 1d — Session continuation history

Goal: preserve one continuous terminal-work history across shell exits while
keeping every OS process, PTY, exit result, audit boundary, and output log
truthful. The UI presents one Session history; internally each continuation is
a distinct immutable Session generation with its own Session ID.

Depends on completed Milestones 1b.E and 1c. Re-read `AGENTS.md`,
`docs/ARCHITECTURE.md`, `docs/SECURITY.md`, the current Session and
ClientRequest fixtures, `docs/progress/milestone-1be.md`, and the bounded output
replay implementation before changing code.

This milestone does not make a finished PTY writable, reuse a Session ID,
survive a daemon restart with the same PTY, execute old terminal output, or add
a new top-level product object. `Session` remains the protocol vocabulary.

## Required design review — stop before tests or implementation

Inspect the current protocol, SQLite representation, replay API, and web
Continue flow. Write one concrete additive proposal in
`docs/progress/milestone-1d.md` covering all of these points:

1. An optional Session lineage field, preferably
   `continued_from: Option<SessionId>`, with a safe `None` default for existing
   JSON and stored Sessions.
2. A dedicated `CONTINUE_SESSION { session_id }` request. Ordinary
   `CREATE_SESSION` must not let a client claim arbitrary lineage.
3. Exact eligibility: the source must exist, have `FULL` capture, be in
   `COMPLETED`, `FAILED`, or `INTERRUPTED`, and have no existing successor.
4. Linear v1 lineage: one Session has at most one direct successor. Define the
   deterministic response to duplicate and concurrent Continue requests and
   how the UI finds the latest generation. Do not silently create branches.
5. Safe inheritance: shell executable, cwd, Project when valid, current client
   dimensions or documented defaults, and one stable display title. Never
   inherit PID, process start identity, exit code, current Command, terminal
   bytes, or an initial Command.
6. Storage migration and uniqueness enforcement, including legacy Session
   JSON, missing ancestors, cycles, cross-Machine references, and deletion or
   retention behavior.
7. How the web obtains and bounds a lineage, replays each generation oldest
   first, and navigates between generations. Prefer the existing authenticated
   bounded per-Session replay resource unless a new HTTP resource is clearly
   necessary.
8. A total client-side history bound in addition to existing per-request
   bounds. Specify maximum generations, total decompressed bytes, partial
   history behavior, and corrupt/missing-segment warnings.
9. Compatibility and generated TypeScript. Show representative JSON for a
   legacy Session, a continued Session, the Continue request, success, and all
   important errors.

Explain whether a successful duplicate request returns the already-created
successor or returns a structured conflict naming it. The behavior must be
safe under a double-click and two concurrent clients.

Then stop and ask the human to approve the proposal. Do not edit protocol
fixtures, migrations, or production code until approval is explicit. If the
design cannot remain additive, report the fixture break and stop.

## Required semantics

- Detaching from a live Session still reattaches to that exact Session and PTY;
  it never creates a generation.
- A finished Session remains immutable and read-only forever.
- Continue always creates a distinct `UI` / `FULL` Session with a new ID and a
  new PTY. The source ID, state, exit result, log, Commands, Events, and Metrics
  remain unchanged.
- Historical output remains in each generation's existing `.log.zst`. Never
  duplicate or append ancestor bytes into a descendant log.
- The combined presentation is a virtual history assembled from lineage. It
  must clearly mark generation boundaries and must never send historical bytes
  to a PTY.
- Saved terminal output is untrusted data. Replay it only through the terminal
  renderer or an explicitly specified safe export path; never interpolate it
  into HTML, a shell, a title, or a Command.
- Input, resize, Ctrl-C, mobile keys, kill, and future signals target only the
  latest live generation. Every historical generation remains read-only.
- The latest finished generation may be continued once. Continuing an older
  generation opens or identifies its existing successor instead of branching.

## 1. Protocol and persistence

After approval:

- Add only the approved additive Session field and ClientRequest variant in
  `crates/protocol`; keep `PROTOCOL_VERSION = 1` if compatibility is preserved.
- Use `serde` defaults so existing Session fixtures and stored JSON decode with
  no lineage.
- Update fixtures by hand, extend every-kind coverage, and regenerate the
  TypeScript contract through `ts-rs`.
- Add a numbered SQLite migration enforcing the approved lineage relationship
  and at-most-one-successor rule. Do not reset or rewrite existing data.
- Store lineage as queryable relational data even if the complete Session JSON
  remains stored. Define one source of truth and test that JSON and columns
  cannot drift.
- Add plain store operations for finding a predecessor, direct successor,
  ordered lineage, and latest generation with cycle and length protection.
- Reject self-reference, cycles, unknown ancestors, cross-Machine lineage,
  non-`FULL` capture, live sources, and a second successor with errors naming
  the relevant Session IDs.

## 2. Atomic daemon continuation

- Put continuation behind one SessionManager operation used by every remote
  interface. Do not implement it as a web-only sequence of list/create calls.
- Validate the source and reserve lineage atomically before exposing success so
  two clients cannot create two descendants.
- Spawn the descendant using only the approved safe metadata. It starts like a
  normal `UI` / `FULL` Session and uses all existing authentication, capture,
  lifecycle, Metrics, logging, and audit boundaries.
- If PTY creation or persistence fails, leave no dangling lineage row or
  half-created Session. The source remains unchanged and replayable.
- Never rerun the source's last Command, current Command, scrollback, OSC
  payload, or terminal output.
- A continuation after daemon restart must work from a persisted finished
  Session even though its PTY handle is absent.
- Do not emit a new Event kind before Milestone 3. Lineage is structured
  Session state, not inferred terminal output.

## 3. Combined history and version navigation

- Replace the temporary router-only `continuedFrom` state with persisted daemon
  lineage. A reload, another browser, or another Machine viewing the daemon
  must see the same relationship.
- Present one stable Session-history title with generation markers such as
  `1 of 3`, timestamps, terminal state, and exit code when known. Do not grow
  repeated `"(continued)"` suffixes.
- Provide an ordered history/timeline for moving among generations. Each
  finished generation can replay its own saved output and Commands.
- The default combined view shows prior output in order with an unmistakable,
  non-terminal-data separator such as `Session ended — continued at ...`, then
  the current generation. Do not inject the separator into a retained log.
- Isolate terminal emulation between generations so ANSI clear-screen,
  alternate-screen, or cursor-control bytes in one generation cannot erase or
  rewrite another generation's presentation.
- Collapse or virtualize older generations when necessary to enforce the
  approved generation and decompressed-byte limits. Show when earlier history
  was omitted; never imply it was deleted.
- Only the latest live generation registers terminal input and resize handlers.
  If every generation is finished, the combined view is fully read-only and
  offers Continue on the latest generation.
- On successful Continue, navigate to and attach the new generation while the
  previous output remains accessible directly above or through the timeline.
- On failure, leave every existing generation and the current view unchanged.

## Tests first

- Protocol: legacy Session JSON defaults to no predecessor; continued Session
  and `CONTINUE_SESSION` fixtures; every-kind coverage; generated TypeScript is
  current.
- Store: migrate an existing database without reset; create and query a
  three-generation lineage; enforce one successor; reject missing ancestor,
  self-reference, cycle, and cross-Machine linkage; detect malformed legacy
  data without looping forever.
- Daemon: Continue accepts every finished FULL state and rejects live or
  reduced-capture Sessions; descendant receives only approved safe metadata;
  source remains byte-for-byte unchanged; old Command is never sent to FakePty.
- Atomicity: simultaneous Continue requests produce exactly one descendant and
  the approved deterministic duplicate result.
- Failure rollback: injected store and PTY spawn failures leave no descendant,
  reservation, process, or audit payload behind.
- Restart: a persisted finished source with no PTY handle can be continued and
  all generations remain replayable.
- Replay: empty, missing, corrupt-final-frame, and bounded-large logs in any
  generation produce per-generation warnings without hiding valid siblings.
- Privacy: historical output and rejected input appear in no request payload,
  title, audit payload, or newly created descendant log.
- Web: reload reconstructs lineage from daemon data; version navigation works;
  separators are UI-only; only latest live generation sends input/resize;
  duplicate clicks create one descendant; the old Command is never resent.
- Playwright: create and finish generation 1, Continue twice across page
  reloads, verify three distinct IDs and ordered retained output, attach/input
  only to generation 3, then inspect generation 1 read-only.

No real sleeps over 100 ms, no network access in fast tests, and no weakening
of Milestone 1b.E lifecycle or replay bounds.

## Done when

A user can treat multiple shell lifetimes as one navigable Session history
without any Session ID, log, process, Command, exit result, or audit boundary
being falsified; lineage survives browser and daemon restart; combined replay
is bounded and visibly segmented; `just check` and Playwright pass; Roadmap 1d
and `docs/progress/milestone-1d.md` are complete. Commit logical changes, send
the required Milestone 1d report, and stop before Milestone 3.
