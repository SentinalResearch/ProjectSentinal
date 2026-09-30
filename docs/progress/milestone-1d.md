# Milestone 1d progress — Session continuation history

Status: complete. The additive design below was approved and implemented.

## Current implementation inspected

- `Session` is stored as JSON in SQLite together with relational `id`, `state`,
  and `updated_at` columns. Existing Session fields added after v1 use Serde
  defaults.
- `CREATE_SESSION` creates an unrelated Session. The web's current Continue
  button calls it with copied shell/cwd/title and remembers the predecessor
  only in React Router state. The relationship disappears on reload.
- `SESSION_UPDATED` is the existing success response for Session creation and
  changes.
- Finished FULL Sessions are immutable at PTY operation boundaries. Their
  output is available through the authenticated, paged
  `GET /api/v1/sessions/{id}/output` resource with a 1 MiB page limit and a
  64 MiB per-Session decompression/scan limit.
- Each Session already has its own numeric output path, Commands, Events,
  Metrics, exit result, and remote-input audit boundary.

## Proposed additive wire contract

Add this field to `protocol::Session`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub continued_from: Option<SessionId>
```

`None` means the Session starts a lineage. `Some(id)` means this Session is the
single direct successor of `id`. It says nothing about shared processes: every
Session remains an independent immutable process/PTY generation.

Add exactly this remote request:

```rust
ClientRequest::ContinueSession { session_id: SessionId }
```

`CREATE_SESSION` does not gain a lineage field. An extra `continued_from`
property on CREATE_SESSION JSON is ignored by current Serde behavior and can
never enter manager/store state. Only `CONTINUE_SESSION` may call the dedicated
continuation operation.

No new success message is necessary. Success, including an idempotent duplicate,
returns the existing `SESSION_UPDATED { session }` response containing the
direct successor. Errors continue to use the bounded existing
`ERROR { message }` shape; continuation messages are fixed/bounded and do not
include paths or terminal data. No terminal bytes, previous Command, or output
appear in either request or response.

`PROTOCOL_VERSION` remains 1. This is additive: legacy Session JSON omits the
field and decodes as `None`; serialization also omits `None`, preserving old
fixture shape. `ts-rs` generates an optional predecessor property for new
clients. Existing CREATE_SESSION JSON remains unchanged and readable.

### Representative JSON

Legacy Session (no lineage field, still readable):

```json
{
  "id": 12,
  "machine_id": "mac-mini",
  "project_id": null,
  "title": "Training shell",
  "shell": "/bin/zsh",
  "cwd": "/Users/owner/project",
  "state": "COMPLETED",
  "current_command": null,
  "origin": "WATCH",
  "capture": "FULL",
  "pid": 4200,
  "process_started_at": null,
  "exit_code": 0,
  "log": null,
  "created_at": 1790000000000,
  "updated_at": 1790000100000
}
```

Continued Session:

```json
{
  "id": 13,
  "machine_id": "mac-mini",
  "project_id": null,
  "title": "Training shell",
  "shell": "/bin/zsh",
  "cwd": "/Users/owner/project",
  "state": "IDLE",
  "current_command": null,
  "origin": "UI",
  "capture": "FULL",
  "pid": 4312,
  "process_started_at": null,
  "exit_code": null,
  "log": null,
  "continued_from": 12,
  "created_at": 1790000200000,
  "updated_at": 1790000200000
}
```

Request and success:

```json
{ "version": 1, "payload": { "type": "CONTINUE_SESSION", "session_id": 12 } }
```

```json
{
  "version": 1,
  "payload": {
    "type": "SESSION_UPDATED",
    "session": {
      "id": 13,
      "machine_id": "mac-mini",
      "project_id": null,
      "title": "Training shell",
      "shell": "/bin/zsh",
      "cwd": "/Users/owner/project",
      "state": "IDLE",
      "current_command": null,
      "origin": "UI",
      "capture": "FULL",
      "pid": 4312,
      "process_started_at": null,
      "exit_code": null,
      "log": null,
      "continued_from": 12,
      "created_at": 1790000200000,
      "updated_at": 1790000200000
    }
  }
}
```

Important failures retain the current error shape:

```json
{ "version": 1, "payload": { "type": "ERROR", "message": "unknown Session 99" } }
```

```json
{ "version": 1, "payload": { "type": "ERROR", "message": "Session 12 capture level COMMANDS cannot be continued; FULL required" } }
```

```json
{ "version": 1, "payload": { "type": "ERROR", "message": "Session 12 is in state RUNNING and cannot be continued" } }
```

```json
{ "version": 1, "payload": { "type": "ERROR", "message": "Session lineage at 12 is invalid: cycle includes Session 9" } }
```

Session ID zero is rejected during request validation. Storage and manager
errors name all relevant source/successor IDs without including terminal data.

## Eligibility, inheritance, and duplicate behavior

A new successor is eligible only when the source:

1. exists on this daemon;
2. has `FULL` capture;
3. is `COMPLETED`, `FAILED`, or `INTERRUPTED`;
4. has a valid, acyclic same-Machine lineage; and
5. has no direct successor.

If a valid direct successor already exists, CONTINUE_SESSION is idempotent:
the daemon returns that existing Session through `SESSION_UPDATED`. It does
not spawn a PTY, allocate another lineage row, or return a conflict. Thus two
concurrent clients and a double-click both receive the same successor ID.
Continuing any older generation likewise identifies its existing direct
successor instead of branching. The UI then follows the persisted chain to
the latest generation.

The descendant always has `origin = UI`, `capture = FULL`, a new Session ID,
new PID, new PTY, new timestamps, no exit code, no current Command, and no
initial Command. It inherits only:

- the source's shell executable, revalidated as non-empty and bounded before
  spawn;
- the source cwd, revalidated as non-empty and bounded;
- the same `project_id` when it is a non-empty bounded identifier; no client
  can replace it during Continue;
- a stable title: the exact existing title when present, otherwise
  `Session <root-id>` for every descendant in that lineage; and
- terminal defaults of 80 columns by 24 rows.

The request deliberately contains no dimensions, matching
`CONTINUE_SESSION { session_id }` exactly and making behavior identical after
a daemon restart. After attach, the web client immediately sends the real
xterm dimensions through the existing RESIZE operation. A missing cwd or
unavailable shell makes the spawn fail cleanly; the daemon does not fall back
to a different shell/directory because that would falsify continuation.

The descendant never inherits PID, process start identity, state, exit code,
current Command, prior argv, terminal bytes, scrollback, adopted-log metadata,
or any initial Command. Existing ancestor rows and files are not updated.

## SQLite migration and source of truth

Migration `002_session_lineage.sql` will add a nullable relational column and
constraints equivalent to:

```sql
ALTER TABLE sessions ADD COLUMN continued_from INTEGER
    REFERENCES sessions(id) ON DELETE RESTRICT
    CHECK (continued_from IS NULL OR continued_from <> id);

CREATE UNIQUE INDEX sessions_one_successor
    ON sessions(continued_from)
    WHERE continued_from IS NOT NULL;
```

The relational `sessions.continued_from` column is authoritative for lineage.
The Session JSON copy must contain the same value. Legacy rows naturally have
SQL `NULL`, and their absent JSON field decodes to `None`. Insert and load paths
compare the two representations; a mismatch is a named corruption error, not
a silent repair. Ordinary `insert_session` accepts only `continued_from = None`.
A dedicated transactional `insert_continuation(source_id, descendant)` writes
the column and matching JSON together. `update_session` may update state and
JSON only after proving its lineage value still equals the immutable column.

Store operations will provide:

- `predecessor(session_id)`;
- `direct_successor(session_id)`;
- `ordered_lineage(session_id)` from root to latest; and
- `latest_generation(session_id)`.

Traversal uses a visited-ID set and a hard daemon limit of 256 generations.
It errors on a self-reference, cycle, missing ancestor/successor, duplicate
successor, cross-Machine edge, JSON/column mismatch, or limit exhaustion. New
edges are checked by walking ancestors inside the same SQLite transaction,
and the partial unique index is the final one-successor concurrency guard.

There is no Session deletion API today. `ON DELETE RESTRICT` keeps an ancestor
from being removed beneath descendants, and output retention remains one
numeric `.log.zst` file per Session. A future deletion feature must define an
explicit whole-lineage or youngest-first policy; Milestone 1d does not delete
or merge rows, Commands, Events, Metrics, audit entries, or logs. Migration
failure on manually corrupted duplicate data is reported and does not reset
the database.

## Atomic daemon continuation

`SessionManager::continue_session(source_id)` is the only continuation
operation used by remote interfaces. It requires persistent storage; a manager
without a Store returns a named unavailable error rather than creating an
ephemeral relationship.

The manager uses one continuation critical section per daemon. Within it:

1. load and validate the complete source lineage;
2. return the existing direct successor immediately when present;
3. validate source state/capture and derive only the safe metadata above;
4. allocate a new ID and stage a new PTY, handle, and empty numeric log without
   publishing them to clients or starting reader/exit tasks;
5. hold the Session-map write guard while the Store transaction atomically
   inserts the descendant and its predecessor column;
6. publish the handle, start its normal reader/exit/metrics paths, and return
   `SESSION_UPDATED` only after the Store commit.

The manager serialization gives deterministic in-process behavior; the SQLite
unique index protects against an unexpected second writer. If that constraint
reports that another successor already won, the staged PTY is killed, its
empty log is removed, and the committed successor is returned.

If spawn fails, nothing was persisted. If persistence fails, the staged child
is killed and its unpublished empty log is removed. Holding the map write guard
across the synchronous Store commit makes publication after commit infallible
with respect to map locking. No reader task, audit input record, lineage
reservation, or visible Session remains after rollback; an allocated numeric
ID may be skipped. The source is never mutated. Continuation after restart uses
the persisted source and does not require an in-memory PTY handle.

No Event kind is added. Existing authentication protects the WebSocket
request, capture/lifecycle gates continue to protect PTY operations, and audit
records remain per-generation metadata about bytes that actually reached a
PTY.

## Web lineage and bounded combined history

No new lineage HTTP route is needed. `LIST_SESSIONS` already returns every
Session on the selected daemon; the optional predecessor field lets the API
adapter build and validate predecessor/successor maps. This works after reload,
from another browser, and after daemon restart. The existing authenticated
per-Session output resource remains the only source of retained bytes.

The web uses these independent bounds:

- at most 256 Session metadata nodes are traversed, matching the daemon
  corruption guard;
- at most 32 generations are mounted/replayed in one history window;
- at most 32 MiB of decompressed saved output is fetched across that window;
  the budget is divided equally among its visible generations, so one large
  log cannot hide all siblings; and
- existing 256 KiB client pages, 1 MiB server pages, and 64 MiB per-Session
  scan limits remain unchanged.

The default window contains the newest 32 generations and renders those bytes
oldest first. When more exist, the UI states how many earlier generations were
omitted from this view and that their data remains saved. Timeline navigation
can move the 32-generation window to include an older selected Session without
loading both windows at once. A generation that reaches its share of the byte
budget is marked `output truncated in this view`; no data is claimed deleted.
A missing, empty, or corrupt-final-frame log produces a warning on only that
generation and never hides valid siblings.

Every displayed generation owns a separate xterm instance. Separators such as
`Session 12 ended — continued as Session 13 at …` are ordinary React UI outside
the xterm hosts and are never written to a retained log. Separate emulators
prevent clear-screen, alternate-screen, and cursor-control bytes in one log
from modifying another generation. Terminal bytes go only to xterm and never
to HTML, a title, CREATE_SESSION, or a shell.

The stable history title comes from the root-derived title rule above. The
timeline shows `generation N of M`, timestamps, state, and known exit code.
Commands are fetched through existing bounded GET_HISTORY requests for the
selected generation, not merged or reassigned.

Only the latest generation can expose controls. If it is live, only its xterm
registers input and resize handlers; Ctrl-C, mobile keys, and kill target that
ID. All ancestors are always read-only. If the latest generation is finished,
the entire history is read-only and Continue is offered on that latest ID.
Older generations show `View successor`, never a branch-creating action.

The temporary router `continuedFrom` state and repeated `"(continued)"` title
logic are removed. CONTINUE_SESSION success navigates to the returned Session,
refreshes LIST_SESSIONS, resolves the latest generation, and attaches only
that generation. Failure leaves the current lineage and view unchanged.

## Planned tasks and proof

- [x] Add the optional Session field and CONTINUE_SESSION request, preserve
  legacy decoding, update fixtures by hand, and regenerate TypeScript —
  protocol unit/contract tests and `just gen-ts --check`.
- [x] Add migration 002, immutable JSON/column consistency, and bounded lineage
  queries — `crates/store/tests/store.rs` migration and lineage tests.
- [x] Implement the single atomic manager continuation path and WebSocket
  dispatch — daemon FakePty/store failure, concurrency, rollback, restart, and
  privacy tests.
- [x] Replace router-only Continue with persisted lineage and bounded isolated
  history panes — adapter, lifecycle, and Session detail Vitest tests.
- [x] Prove three generations across reload with input only on generation 3 —
  Playwright.
- [x] Run `just check` and Playwright; update Roadmap/report only after all
  acceptance criteria pass.

## Resolved decisions

- The human approved the additive contract, idempotent duplicate success,
  80x24 continuation default, and the 256/32-generation plus 32 MiB web bounds.
- Protocol version 1 remains unchanged and no dependency was added.
- Migration 002 preserves existing rows; the relational predecessor column is
  authoritative and is checked against the stored Session JSON on every read
  and update.

## Verification

- Protocol: legacy and continued Session fixtures, every-kind coverage, and
  generated TypeScript are current.
- Store/daemon: migration, lineage corruption, atomic duplicate, rollback,
  restart, safe inheritance, and output privacy tests pass.
- Web: bounded lineage, isolated replay, selected-generation Commands, and
  lifecycle tests pass.
- Playwright: three distinct generations survive two reloads; only generation
  3 sends terminal input.
