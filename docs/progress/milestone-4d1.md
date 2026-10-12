# Milestone 4.D.1 progress — Incremental aggregation and control read model

Status: **DESIGN REVIEW — AWAITING HUMAN APPROVAL.** No protocol fixture,
migration, production code, or real Machine data has been changed. The
proposal below is additive, needs no dependency, and keeps
`PROTOCOL_VERSION = 1`.

## Plan and proving tests

1. [x] Measure the existing browser request graph with the current fake
   Machine sockets and request counters.
2. [x] Review the authority, privacy, link, storage, browser, freshness,
   compatibility, rollback, and performance boundaries.
3. [ ] Obtain explicit approval for this proposal, especially the bounded
   storage of owner-authored Session titles on the control.
4. [ ] Add request-count regression tests, then repair the direct-query path
   (`web/src/data/api/index.test.ts`, `LiveDataContext.test.tsx`).
5. [ ] Add additive projection HTTP/protocol contracts and generated
   TypeScript (`crates/protocol/tests/contract.rs`, hand-written fixtures,
   `just gen-ts --check`).
6. [ ] Add migration 007 and worker/control store behavior
   (`crates/store/tests/projection.rs`).
7. [ ] Add worker outbox, authenticated ingress, compaction, resync, and
   isolation (`crates/daemon/tests/projection_sync.rs`, control tests).
8. [ ] Add the owner snapshot/live stream and PWA cutover (web adapter,
   service-worker privacy, accessibility, and production Playwright tests).
9. [ ] Run `just check`, production Playwright, the scale fixture, and the
   available real-Machine acceptance; update Roadmap and this report.

## Baseline measurement

The existing fake in `web/src/data/api/index.test.ts` already counts every
`ClientRequest` in `FakeSocket.requests`. A temporary, uncommitted counter was
run against the existing two-Machine fixture, then removed. The targeted test
passed.

For two Machines with one Session each, the cold calls were:

```text
control: GET_MACHINE, GET_MACHINE, GET_MACHINE, LIST_SESSIONS, GET_EVENTS
worker:  GET_MACHINE, GET_MACHINE, LIST_SESSIONS, GET_EVENTS
HTTP:    GET /api/v1/machines twice
WS:      one proxied socket per Machine
```

That is 9 WebSocket request/reply operations, 2 registry HTTP requests, and 2
Machine sockets. The extra control `GET_MACHINE` is initialization. Calling
`getMachines()` and `getSessions()` together then independently refreshes
every Machine, duplicating `GET_MACHINE`.

One fake-clock two-second interval produced:

```text
control: GET_MACHINE, LIST_SESSIONS, GET_EVENTS
worker:  GET_MACHINE, LIST_SESSIONS, GET_EVENTS
HTTP:    GET /api/v1/machines once
```

Let `M` be eligible Machines, `S_i` their Session counts, and
`S = sum(S_i)`. The current request graph is:

| Operation | Current count |
|---|---:|
| Cold control identity | `1 GET_MACHINE` |
| Cold registry loads | `2 HTTP GETs` |
| Cold Machine refresh | `2M GET_MACHINE` |
| Cold Session refresh | `M LIST_SESSIONS` |
| Cold Event replay | `S GET_EVENTS(after=null, limit=500)` |
| Cold sockets | `M` control-origin proxied WebSockets |
| Every two-second tick | `1 registry GET + M GET_MACHINE + M LIST_SESSIONS + S GET_EVENTS` |

Thus cold data work is `3 + 3M + S` HTTP/WS operations plus `M` socket
upgrades. Steady state is `1 + 2M + S` operations every two seconds. At three
Machines and 30 Sessions this is 42 cold operations and 37 operations per
tick: 1,110 per minute and 5,550 over five idle minutes. Every Event request
starts again at `after: null`, so the browser transfers and parses up to the
first 500 Events per Session on every tick merely to deduplicate them in
memory.

Other explicit screens currently add:

- history/search: one bounded request per selected Machine (or `M` for all);
- Recipes: `M LIST_RECIPES` requests;
- notification Settings: `M GET_NOTIFICATION_JOURNAL` plus two independent
  `M GET_NOTIFICATION_RULES` passes for rules and status;
- Machine details/Metrics: one owning-Machine request each, on demand;
- saved output: bounded pages from the owning worker, on demand.

Machines are queried with concurrency three, but Events within each Machine
are fetched sequentially. A slow Session therefore holds up that Machine's
complete snapshot. Reload loses all remote Session state because it exists
only in browser memory.

## Approved shape proposed

```text
authoritative worker transaction
  -> bounded SQLite projection outbox
  -> authenticated existing Machine link
  -> idempotent control projection transaction
  -> one owner snapshot + one control-origin live stream
  -> PWA in-memory dashboard

owner terminal action
  -> control route
  -> current identity proof / authoritative worker
  -> worker capability + lifecycle authorization
  -> PTY
```

The projection is a disposable dashboard index. It never owns a PTY, never
authorizes a mutation, and never becomes a recovery source for worker data.

## 1. Authority and projected data

Workers remain authoritative for Machines' runtime facts, Sessions, Commands,
Events, Metrics history, output logs, PTYs, Recipes, and every mutation. Each
projected record carries `source_machine_id`, `source_epoch`,
`source_cursor`, `observed_at`, and control-clock `synced_at`. Source cursors,
not timestamps, order updates.

The control stores only these projection classes:

| Class | Fields | Dashboard need |
|---|---|---|
| Machine | immutable ID; registry display name; OS; daemon version; declared capabilities; registry/link status; last seen | Machine cards and routing label |
| Session | local ID; optional predecessor ID; bounded owner-authored title; state; origin; capture; created/updated/finished times; exit code | cards, lineage marker, capture/state truth |
| Capture display capabilities | attach/read-screen, input, resize, daemon-kill, Commands, output/logs, Metrics | explain available UI without granting authority |
| Health summary | progress; activity/resources/stability/overall enums; error count | deterministic health card |
| Safe Event summary | Event ID; Session ID; time; closed category; numeric exit/duration/progress/resource fields from an allow-list | last Event and compact timeline |
| Latest Metrics | sampled time; aggregate CPU/RAM/GPU/VRAM percentages/bytes | current dashboard bars only |

Health `evidence` strings are not projected. Event messages, prompts, paths,
URLs, process names, and free-form units are not projected. Progress units are
normalized to `PERCENT`, `EPOCH`, `STEP`, `ITEM`, or `UNKNOWN`; resource names
to `CPU`, `RAM`, `GPU`, `VRAM`, or `UNKNOWN`. `ERROR`, `WARNING`, `WAITING`,
`STALLED`, `CHECKPOINT`, and `SERVER_STARTED` retain only the category and
safe numeric fields, never their source text.

The control does **not** store terminal bytes, compressed output, terminal
input, scrollback, Command text, cwd, shell, PID/process tree, environment,
arbitrary filesystem paths, adopted-log contents, full Events, Health
evidence, Project ID/name, Recipe values, browser subscriptions, or Machine
credentials in projection rows. The current Project field is omitted because
the protocol does not yet distinguish a safe opaque ID from a path-like or
owner-sensitive label. Existing control-owned credential/subscription tables
remain separate.

One deliberate privacy tradeoff needs approval: Session title is useful on
the dashboard but can contain owner-written sensitive text. It is accepted
only as UTF-8 without controls, at most 48 Unicode scalars and 192 bytes. A
missing or invalid title is displayed as `Session <id>`. It is never derived
from terminal output or Command text. Approval of this design explicitly
approves storing this bounded title in the control backup; rejecting it means
the projection will store only `Session <id>`.

## 2. Incremental worker/control contract

No new daemon or transport is introduced. The worker POSTs to
`/api/v1/link/projection` with its existing worker credential. The route caps
the body at 256 KiB before JSON decoding, authenticates the credential, derives
the Machine ID, then compares it with the claim before any row change.

`LinkHeartbeatRequest` and `LinkHeartbeatResponse` gain optional
`projection_sync` fields with `serde` defaults. Unknown struct fields are
ignored by older peers, unlike adding an enum capability that an old control
could reject. Absence means unsupported. Existing fixtures remain readable.

The new wire types are `ProjectionSyncReport`, `ProjectionSyncDirective`,
`ProjectionBatch`, `ProjectionChange`, `ProjectionAcknowledgement`,
`ProjectedMachine`, `ProjectedSession`, `ProjectedHealth`,
`ProjectedEventSummary`, `ProjectedMetrics`, `ProjectionFreshness`,
`ControlProjectionSnapshot`, and `ControlProjectionMessage`. They live only in
`crates/protocol`, derive the normal wire traits and `TS`, and receive
hand-reviewed fixtures. Browser-only presentation types remain in the web
adapter.

Worker heartbeat capability:

```json
{
  "machine_id": "01M40Q6XWORKER",
  "sequence": 82,
  "sent_at": 1791792000000,
  "endpoint": {"address":"100.66.29.67","port":7420,"dns_name":"worker.tailnet.ts.net"},
  "daemon_version": "0.4.1",
  "capabilities": ["SESSIONS","EVENTS","METRICS","ADOPT"],
  "projection_sync": {
    "schema_version": 1,
    "source_epoch": "pe_01M4D1",
    "current_cursor": 44,
    "last_acked_cursor": 41
  }
}
```

Control directive:

```json
{
  "status": "ACTIVE",
  "display_name": "MacBook",
  "next_heartbeat_ms": 30000,
  "refresh_due": false,
  "notification_ingress": true,
  "projection_sync": {
    "enabled": true,
    "schema_version": 1,
    "committed_cursor": 41,
    "max_batch_bytes": 262144,
    "max_changes": 128,
    "action": "UPLOAD"
  }
}
```

A delta contains at most 128 changes and 256 KiB:

```json
{
  "schema_version": 1,
  "machine_id": "01M40Q6XWORKER",
  "source_epoch": "pe_01M4D1",
  "mode": "DELTA",
  "base_cursor": 41,
  "through_cursor": 44,
  "sent_at": 1791792000200,
  "changes": [
    {
      "cursor": 42,
      "type": "SESSION_UPSERT",
      "session": {
        "id": 128,
        "continued_from": null,
        "title": "Qwen Training",
        "state": "RUNNING",
        "origin": "WATCH",
        "capture": "FULL",
        "created_at": 1791791900000,
        "updated_at": 1791792000100,
        "finished_at": null,
        "exit_code": null,
        "health": {"progress":68.0,"activity":"GOOD","resources":"GOOD","stability":"GOOD","errors":0,"overall":"HEALTHY"}
      }
    },
    {
      "cursor": 43,
      "type": "EVENT_APPEND",
      "event": {"id":902,"session_id":128,"at":1791792000150,"category":"PROGRESS","current":68.0,"total":100.0,"unit":"EPOCH"}
    },
    {
      "cursor": 44,
      "type": "METRICS_REPLACE",
      "metrics": {"session_id":128,"at":1791792000180,"cpu_percent":31.0,"rss_bytes":4294967296,"gpu_percent":96.0,"vram_used_bytes":21400000000}
    }
  ]
}
```

Acknowledgement after the control transaction commits:

```json
{"status":"ACCEPTED","source_epoch":"pe_01M4D1","committed_cursor":44}
```

The same batch after a lost acknowledgement returns `DUPLICATE` with cursor
44. A gap, unknown epoch, control rollback, or validation failure changes no
projection row and asks for a full resynchronization:

```json
{"status":"RESYNC_REQUIRED","source_epoch":"pe_01M4D1","committed_cursor":38,"reason":"CURSOR_MISMATCH"}
```

A tombstone is an ordered change, not a missing list entry:

```json
{"cursor":45,"type":"SESSION_TOMBSTONE","session_id":128,"deleted_at":1791792010000}
```

Bounds fail before partial application:

```json
{"code":"PROJECTION_TOO_LARGE","message":"projection batch exceeds 262144 bytes","max_bytes":262144}
```

Full resync uses the same endpoint with `mode: "SNAPSHOT"`, a random bounded
`snapshot_id`, fixed `snapshot_cursor`, zero-based `chunk_index`, and `final`.
The control stages at most one snapshot per Machine (2,000 Sessions, 10,000
safe Events, 16 MiB, 15 minutes). Only the final validated chunk atomically
replaces that Machine's projection and cursor. Deltas after
`snapshot_cursor` remain in the worker outbox and upload afterward. Clock skew
may mark source times suspect but never changes cursor order.

Representative final snapshot chunk:

```json
{
  "schema_version": 1,
  "machine_id": "01M40Q6XWORKER",
  "source_epoch": "pe_01M4D1",
  "mode": "SNAPSHOT",
  "snapshot_id": "ps_01M4D1SYNC",
  "snapshot_cursor": 52,
  "chunk_index": 3,
  "final": true,
  "records": [
    {
      "type": "SESSION_UPSERT",
      "session": {
        "id": 128,
        "title": "Qwen Training",
        "state": "COMPLETED",
        "origin": "WATCH",
        "capture": "FULL",
        "created_at": 1791791900000,
        "updated_at": 1791792020000,
        "finished_at": 1791792020000,
        "exit_code": 0
      }
    }
  ]
}
```

## 3. Worker outbox

Migration 007 adds `projection_state` and `projection_outbox` on every daemon.
The worker creates a projection change in the same SQLite transaction as the
authoritative structured state whenever the store operation already owns that
transaction. Callers that currently compose operations gain one store
transaction rather than a second terminal-output parser.

Every authoritative change increments a per-Machine `source_cursor`.
Unacknowledged replaceable rows (`MACHINE_REPLACE`, Session metadata/Health,
and latest Metrics) are upserted by entity key with the newest cursor.
Lifecycle transitions, terminal states, safe Event summaries, and tombstones
are ordered critical rows and are never compacted individually. Metrics are
one latest Machine row plus one latest row per Session, so a two-second sampler
cannot grow the queue indefinitely.

The outbox is capped at 20,000 rows and 16 MiB, with 25% reserved for critical
rows. It first replaces Metrics, then non-lifecycle Session summaries. If a
critical row still cannot fit, authoritative local work continues only if its
own transaction is valid, `snapshot_required` is set, and a visible
`PROJECTION_OVERFLOW` diagnostic is recorded; the control is never told it has
a complete delta. The next connection performs a full snapshot. A terminal
state therefore cannot disappear silently even under pressure.

Uploads are wake-driven with a 250 ms coalescing window, at most four requests
per second and one in flight per Machine. Retry delays are 1 s, 5 s, 30 s,
2 min, then 5 min. Rows are deleted only with `cursor <= committed_cursor`;
a newer replacement written during an upload survives its acknowledgement.
Rows older than seven days force a snapshot rather than replaying stale
detail. Shutdown drains for at most two seconds. Unpaired, disconnected,
revoked, wrong-control, or expired links retain authoritative local state and
cannot upload.

## 4. Control schema, atomicity, and retention

Migration 007 adds these separate projection tables:

```text
projection_machine_state      PK machine_id; epoch/cursors/schema/freshness/view_revision
projection_sessions           PK (machine_id, session_id); indexed state/updated/view_revision
projection_event_summaries    PK (machine_id, event_id); indexed session/time/view_revision
projection_metrics_latest     PK (machine_id, scope_key); optional session_id; view_revision
projection_tombstones         PK (machine_id, entity_kind, entity_id); view_revision
projection_snapshot_stage     PK (machine_id, snapshot_id, chunk_index); bounded/expiring
projection_global_state       singleton next_view_revision
projection_state/outbox       worker-side tables described above
```

Safe typed JSON is used only for closed Health/Event/Metrics substructures;
identity, state, capture, cursors, times, and relationships are relational
columns. One transaction authenticates Machine binding, validates every
record, applies a complete delta, advances the source cursor, allocates one
global `view_revision`, and commits. A claimed Machine can touch only rows
under the Machine ID derived from its credential.

Limits and retention:

- at most 100 registry Machines and 2,000 projected Sessions per Machine;
- all non-terminal Sessions retained; terminal summaries pruned after 30 days
  and oldest first above 2,000, without touching worker data;
- at most 32 safe Event summaries per Session, 10,000 per Machine, and seven
  days, whichever bound is reached first;
- latest Metrics only, with no time-series cache;
- tombstones kept 30 days so reconnecting browsers cannot resurrect rows;
- snapshots bounded as described above; partial staging is never visible.

Missing lineage ancestors set `lineage_incomplete` instead of inventing a
Session. Self/cyclic lineage makes that Machine projection invalid and asks
for resync; traversal is always length-bounded. Cross-Machine lineage is
unrepresentable in the worker payload and rejected if smuggled. A Session
tombstone removes its summary, Event summaries, and Metrics atomically.

Offline and disconnected Machines retain the last projection. Revocation
immediately purges projected Sessions, Events, and Metrics and retains only
the existing secret-free registry tombstone/status; it cannot receive future
updates. A restored older control asks the worker to replay or snapshot. A
worker cursor regression with the same valid link requires an authenticated
full snapshot, is audit-labelled `SOURCE_CURSOR_REGRESSION`, and atomically
replaces only that Machine. Newer unsupported schemas leave the last known
projection stale and isolate the Machine.

The control Machine's local authoritative store feeds the same validation and
apply function in-process. It does not make a loopback HTTP request and does
not use a special projection schema. HTTP ingress and the local caller are two
entry paths into one plain apply function; no new trait is needed.

## 5. Owner snapshot and live stream

The authenticated browser contract is:

```text
GET /api/v1/control/snapshot
WS  /api/v1/control/live
```

Both require the owner Secure HttpOnly Session and exact allowed Origin. The
snapshot is capped at 2 MiB, 100 Machines, and the 1,000 most relevant Session
summaries (all live first, then newest terminal). It states omitted counts; an
owner explicitly loads older/detail data from the worker. It includes one
control `view_revision`.

Representative snapshot fragment:

```json
{
  "schema_version": 1,
  "view_revision": 992,
  "generated_at": 1791792001000,
  "machines": [{
    "machine_id": "01M40Q6XWORKER",
    "display_name": "MacBook",
    "status": "ONLINE",
    "freshness": "STALE",
    "last_seen": 1791791990000,
    "observed_at": 1791791980000,
    "synced_at": 1791791980500,
    "source_cursor": 44,
    "sessions": [{"id":128,"title":"Qwen Training","state":"RUNNING","capture":"FULL"}],
    "omitted_sessions": 0
  }],
  "truncated": false
}
```

After reading the snapshot, the browser sends
`{"type":"SUBSCRIBE","after_revision":992}` as the first live frame. The
control registers the subscriber before querying rows newer than 992, so a
commit cannot fall into a race. Current row revisions plus retained tombstones
provide bounded catch-up; intermediate replaceable states may coalesce, but
the newest state and every retained safe Event remain. Live messages are:

```json
{"type":"PROJECTION_DELTA","from_revision":992,"to_revision":995,"changes":[...]}
```

If more than 1,000 changes or 2 MiB are needed, the stream sends:

```json
{"type":"SNAPSHOT_REQUIRED","current_revision":1210,"reason":"DELTA_BOUND_EXCEEDED"}
```

The browser then performs one new snapshot request. It keeps one immutable
in-memory projection per Machine and replaces only with a newer cursor. It
opens no worker dashboard socket. Reload and logout clear memory; responses
use `Cache-Control: no-store` and never enter Service Worker Cache Storage,
localStorage, URLs, analytics, or browser logs.

Terminal attach, output replay, Session detail Events, history/search,
Recipes, notification history, and Metrics history remain explicit routes to
the owning worker. Opening a terminal/detail establishes the existing routed
worker socket and first obtains the current authoritative Session before UI
input/resize/kill/Continue controls become enabled. Recipe launch similarly
refreshes the selected worker's Recipe catalog. The mutation itself always
rechecks worker capture/lifecycle rules; a projection value can never grant a
capability.

## 6. Freshness and offline truth

The worker heartbeat reports its current projection cursor even when nothing
changed. The control computes transport freshness without rewriting the
worker's Session state:

- `LIVE`: active unexpired link, heartbeat within 45 s, supported schema, and
  committed cursor equals the latest reported cursor;
- `STALE`: heartbeat still within the existing 90 s online window, but sync is
  over 10 s behind or the projection/schema is incomplete;
- `OFFLINE`: no accepted heartbeat for 90 s;
- `DISCONNECTED`: worker explicitly disconnected;
- `AUTH_EXPIRED`: link credential is past its expiry;
- `REVOKED`: owner revoked it; Session projection has been purged.

Every card shows `synced_at`; stale/offline Metrics stop animating and are
labelled “last sampled …”. The last complete projection stays readable for
offline/disconnected Machines. All live actions fail closed when no current
authoritative route can be proven.

## 7. Immediate direct-query repair and old-worker fallback

Before projection cutover, `getMachines()` and `getSessions()` share one
coalesced initialization promise returning both arrays. Timer, visibility,
route, and manual refreshes share one in-flight refresh per Machine.
Dashboard initialization performs no Session Event requests: deterministic
Health already exists on Session; Event detail loads only when opened.

For detail views, the adapter records the largest accepted Event ID per
Machine/Session and uses `GET_EVENTS { after, limit }`. It fetches at most five
100-Event pages per user request and labels additional history partial. A
reconnect fills a bounded missing range or asks for a fresh detail snapshot;
it never silently returns to `after: null` every two seconds. History/search
gain additive optional `before_command_id` cursors and `next_before_command_id`
responses, defaulting safely for old JSON. Output keeps its existing byte
offset. Fixed configuration catalogs retain hard caps; notification/history
lists use their existing or additive cursors.

The two-second reconciliation interval is removed. Live messages are primary;
a jittered 60-second integrity check refreshes registry/Machine/Session lists
only, is visibility-coalesced, has concurrency three, and never replays Events.

An enrolled old worker without `projection_sync` remains usable in explicit
`LEGACY_DIRECT` mode. The browser makes one initial `GET_MACHINE` plus
`LIST_SESSIONS`, keeps its existing routed live socket, and performs the same
60-second bounded integrity check. It does not restore per-Session Event
polling. The UI labels it “Update worker for offline dashboard”. This
compatibility exception disappears from the critical path as each worker is
updated; an all-current deployment uses only one snapshot and one control
live stream.

## 8. Security, privacy, audit, and backup

- Worker ingress uses the existing directional link credential and validates
  body size before allocation/decoding. One worker cannot name or overwrite
  another Machine.
- Owner snapshot/live routes require the owner cookie, exact Origin, bounded
  output, `no-store`, `nosniff`, and the existing CSP/frame policy.
- Audit rows contain only Machine ID, epoch, cursor range, change count, byte
  count, result class, and time. They contain no projected payload.
- Titles remain potentially sensitive owner metadata; control backups now
  contain the approved bounded projection tables. Docs will name this.
- Logout closes the control stream and destroys browser memory. Revocation
  purges that Machine's projection as defined above.
- Service Worker code and tests continue to reject API/WS/auth/terminal data
  from Cache Storage.
- Terminal/output routes, worker credentials, browser cookies, Push endpoint
  secrets, and VAPID keys do not enter projection types or responses.

## 9. Performance budgets

Deterministic counters, not wall-clock assertions, enforce:

- synchronized dashboard cold/warm load: exactly one bounded control snapshot
  request plus one control-origin live connection;
- critical render: zero worker calls, zero per-Session Event calls, and no
  count growth as Sessions increase from 1 to 1,000;
- idle browser: zero periodic full refreshes and zero periodic Event reads;
- worker upload: at most 128 changes/256 KiB, one in flight, four starts/s;
- browser catch-up: at most 1,000 changes/2 MiB, otherwise one resnapshot;
- direct legacy/detail fan-out: concurrency three, one in flight per Machine;
- integrity reconciliation: no more often than 60 s and no Event replay.

The required 10-Machine/1,000-Session fixture uses injected 150 ms worker
latency and must still record one snapshot, one live stream, zero Event calls,
and zero worker calls before first render.

## 10. Test-first matrix after approval

- Adapter counters first lock in the old N+1 graph, then prove one shared
  initialization, cursored Events, coalesced timer/visibility/manual refresh,
  bounded concurrency, and absence of a two-second replay loop.
- Protocol contracts cover capability negotiation, delta, duplicate
  acknowledgement, tombstone, snapshot chunks, bounds, resync, stale Machine,
  legacy defaults, every-kind coverage, and generated TypeScript.
- Migration/store tests upgrade a populated migration-006 database without a
  reset and cover transaction/cursor atomicity, compaction, retention,
  tombstones, corrupt/newer rows, restart, and rollback preservation.
- Sync tests cover lost acknowledgement, duplicate/reordered/gapped batches,
  control and worker restart/offline, full resync, cursor regression,
  high-frequency Metrics coalescing, overflow, clock skew, and Machine
  isolation without sleeps.
- Security/privacy tests cover wrong/revoked credentials, claimed identity
  mismatch, oversized bodies, cross-Machine overwrite, audit redaction, and
  planted forbidden values across projection rows/responses/logs.
- Web tests cover snapshot-first render, ordered deltas, resnapshot, all six
  freshness states, partial history, live-action gates, reconnect, logout, and
  absence from Cache Storage/localStorage.
- The scale fixture uses 10 Machines, 1,000 Sessions, and injected 150 ms
  worker latency; critical render must remain one snapshot plus one stream.
- Production Playwright uses one control and two fake workers, takes one
  worker offline, reloads from both projections, applies a healthy delta, and
  proves terminal actions still route only to the authoritative owner. Its
  request capture must contain no mock data, worker origin/credential, or
  per-Session Event fan-out.

## 11. Compatibility and rollback

All new fields are optional or new HTTP resources/types, all existing JSON
fixtures remain readable, generated TypeScript is regenerated, and
`PROTOCOL_VERSION` remains 1. The migration is additive; old binaries ignore
the new tables.

Mixed versions negotiate through optional heartbeat fields. A runtime
`control_read_model = false` setting disables sync/snapshot use and selects the
repaired 60-second direct path; it does not drop tables. Rolling back binaries
therefore leaves worker Sessions/Commands/Events/Metrics/logs and control
projection rows untouched. Direct terminal routing remains available whenever
the owning worker is live, even if projection data is absent, corrupt, stale,
or disabled. Cached state never becomes authoritative during rollback.

No dependency outside `docs/DEPENDENCIES.md` is required: existing Tokio,
Axum, Rusqlite, Serde, WebSocket, and tracing facilities cover the design.

## Approval points

Approval of this design approves:

1. storing the bounded owner-authored Session title and the exact safe field
   classes above in the control SQLite database and its backups;
2. additive optional heartbeat fields, projection HTTP/browser resources,
   history cursors, fixtures, and TypeScript while keeping protocol version 1;
3. the 256-KiB/128-change link batch, 20,000-row/16-MiB worker outbox,
   2-MiB/1,000-Session browser snapshot, and stated retention limits;
4. immediate projection purge on Machine revocation, but last-known retention
   for offline/disconnected Machines;
5. a 60-second bounded direct integrity check and labelled old-worker fallback;
6. the migration/rollback and source-cursor regression behavior above.

## Real-Machine acceptance fields

To be filled only against the exact candidate commit; do not record secrets.

| Role | Machine ID | OS/version | Tailscale IP/version | Candidate | Result |
|---|---|---|---|---|---|
| Control | pending | pending | pending | pending | pending |
| macOS worker | pending | pending | pending | pending | pending |
| Linux worker | currently unavailable | pending | pending | pending | pending |

Request/byte evidence to record:

| Scenario | Before requests/bytes | After requests/bytes | Result |
|---|---:|---:|---|
| Cold load | pending | pending | pending |
| Warm reload | pending | pending | pending |
| Five idle minutes | pending | pending | pending |
| One worker offline/reconnect | pending | pending | pending |

## Open questions

- Human approval of the six explicit approval points above.
- Linux real-Machine availability for final two-worker evidence.
- Whether the owner wants Session titles omitted despite the usability cost;
  omission requires no wire redesign and is the privacy-stricter fallback.
