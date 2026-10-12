# MILESTONE 4.D.1 — Incremental aggregation and control read model

Goal: the authenticated control PWA opens quickly and remains current across
several enrolled Machines without polling every Session and replaying its
Events every two seconds. The worker remains authoritative; the control keeps
only a bounded, privacy-minimized materialized read model for dashboard use.

Depends on completed Milestones 4.A and the implemented portion of 4.D. Reuse
the existing Machine registry, authenticated Machine link, namespaced web
identities, retry conventions, SQLite migrations, and control-origin PWA. This
milestone does not cache terminal HTTP responses, copy terminal logs, move PTY
ownership, make cached state authorize an operation, or add a cloud database,
message broker, analytics service, or new dependency.

Before changing code, read `AGENTS.md`, `docs/ARCHITECTURE.md`,
`docs/SECURITY.md`, `docs/TESTING.md`, `docs/DEPENDENCIES.md`,
`docs/progress/milestone-4a.md`, `docs/progress/milestone-4d.md`, the current
control proxy and Machine-link code, `web/src/data/api/index.ts`, and
`web/src/data/LiveDataContext.tsx`. Add Milestone 4.D.1 to
`docs/ROADMAP.md` and create `docs/progress/milestone-4d1.md` with tasks,
proving tests, measured request counts, real-Machine acceptance fields, and
open questions.

## Required design review — stop before tests or implementation

Measure the current request graph first with fakes and counters, then present
one concrete additive design covering all of these points:

1. **Baseline.** Show the current cold-load and steady-state request counts as
   functions of Machines and Sessions. Include `GET_MACHINE`,
   `LIST_SESSIONS`, `GET_EVENTS`, history/search, notification status, and the
   two-second reconciliation loop. Identify duplicate initialization requests,
   sequential per-Session work, and full Event replay.
2. **Authority.** Workers remain the source of truth for Sessions, Commands,
   Events, Metrics, output logs, PTYs, and mutations. The control read model is
   explicitly a projection with `source_machine_id`, source revision/cursor,
   `observed_at`, and `synced_at`. Cached state must never approve input,
   resize, signal, kill, Continue, Recipe execution, or attach.
3. **Data classification.** List every proposed projected field and why the
   dashboard needs it. Default to Machine identity/status, bounded Session
   summary, capture capabilities, deterministic Health/progress, latest safe
   Event summaries, and coalesced latest Metrics. Do not duplicate raw terminal
   bytes, compressed output, input, environment, arbitrary filesystem paths,
   adopted-log contents, browser/Machine credentials, or full Command text/cwd.
   Stop for human approval before storing any additional sensitive class.
4. **Incremental contract.** Define bounded authenticated worker → control
   snapshot/delta batches, per-Machine monotonic cursors, idempotency,
   acknowledgements, tombstones, ordering, full resynchronization, clock skew,
   and schema/capability versioning. Show representative JSON. Prefer the
   existing Machine link and worker outbox pattern; do not create another
   daemon or transport.
5. **Worker outbox.** Specify persistence, byte/row limits, compaction,
   restart replay, overflow behavior, and priority. Lifecycle and terminal
   states cannot be silently dropped. High-frequency Metrics must be
   coalesced; repeated snapshots must not grow the queue without bound.
6. **Control schema.** Show numbered SQLite migrations, primary/foreign keys,
   per-Machine uniqueness, indexes, retention, maximum Machines/Sessions/Event
   summaries, and atomic replacement rules. Define behavior for missing
   ancestors, deleted Sessions, Machine revocation, reconnect, identity reset,
   database rollback, and corrupt or newer projection data.
7. **Browser contract.** Define one bounded initial control snapshot and one
   control-origin live stream for projected changes. The browser must not open
   one dashboard WebSocket per worker or issue one Event request per Session.
   Terminal attach/output replay and explicit detail/history operations remain
   separately routed to the owning worker. Show HTTP/JSON and generated
   TypeScript impact if protocol types are added.
8. **Freshness and offline truth.** Define `LIVE`, `STALE`, `OFFLINE`,
   `DISCONNECTED`, `AUTH_EXPIRED`, and `REVOKED` presentation without rewriting
   worker Session state. Specify staleness thresholds and show last-synced time.
   An offline worker's last complete projection remains readable but all live
   actions fail closed.
9. **Immediate query repair.** Before cutover, use existing Event cursors,
   coalesce concurrent refreshes, eliminate duplicate `GET_MACHINE` calls,
   avoid full Event replay, bound parallelism, and load history/details only on
   demand. Define a compatibility fallback for an enrolled old worker that has
   no projection-sync capability, without restoring the two-second N+1 loop.
10. **Security and privacy.** The PWA service worker continues to cache only
    immutable app-shell assets—never authenticated API data. Define owner
    authorization, Machine binding, payload/body limits, audit metadata,
    redaction, backup implications, logout behavior, and why one worker cannot
    overwrite another Machine's projection.
11. **Performance budgets.** Use deterministic operation counts and injected
    latency rather than flaky wall-clock assertions. After synchronization, a
    dashboard load should need one bounded snapshot request and one
    control-origin live connection, with zero per-Session Event requests and
    zero worker round trips on the critical render path. Steady state must have
    no two-second full refresh. Define bounded batch and reconnect budgets.
12. **Compatibility and rollback.** Prefer additive protocol/HTTP changes and
    keep `PROTOCOL_VERSION = 1` when old fixtures remain readable. Define
    mixed-version behavior, migration rollback, disabling the read model, and
    how direct routing continues when projection data is absent. A rollback
    must not delete worker data or make a cached record authoritative.

Show the complete flow:

```text
worker transaction → bounded sync outbox → authenticated Machine link
→ idempotent control projection → one owner snapshot/live stream → PWA

owner terminal action → control route → authoritative worker → PTY
```

Include representative snapshot, delta, acknowledgement, tombstone, stale
Machine, capability negotiation, bounds error, and resync JSON. Then stop for
explicit human approval. Do not edit protocol fixtures, migrations, production
code, or real Machine data before approval. Stop if the proposal requires a
dependency outside the approved stack or breaks an existing fixture.

## 1. Repair the current direct-query path

After approval:

- Add request-count regression tests before changing the adapter.
- Make initialization one shared operation. `getMachines()` and
  `getSessions()` must not launch duplicate Machine refreshes when called
  together.
- Track the last accepted Event ID per Machine/Session and request only later
  Events. A reconnect may request a bounded missing range or an explicit full
  snapshot; it must not silently reread the first 500 Events forever.
- Coalesce overlapping timer, visibility, route-load, and manual refreshes.
  Only one refresh per Machine may be in flight.
- Replace the two-second full reconciliation loop with live updates plus the
  approved slower bounded integrity check. Use injected clocks and deterministic
  backoff.
- Fetch independent Session deltas concurrently only up to the approved limit.
  A slow Session or Machine cannot block another Machine's already available
  snapshot.
- Fetch history, search, detailed Metrics, Recipes, notification history, and
  saved output only when their UI requests them. Paginate and bound every
  response.

## 2. Worker projection outbox

- Create projection changes in the same logical transaction as their
  authoritative structured state whenever possible. Never infer them by
  scraping terminal output a second time.
- Persist and compact an ordered per-Machine outbox. Acknowledgement means the
  control durably committed the projection cursor, not that a browser rendered
  it.
- Coalesce replaceable Session summaries and latest Metrics while preserving
  terminal lifecycle transitions, safe Event summaries, and tombstones.
- Upload outside PTY, watcher, Metrics, and request hot paths. Bound batches,
  attempts, backoff, concurrency, retention, and shutdown drain time.
- An unpaired, revoked, or wrong-control worker keeps its authoritative local
  state and cannot upload projection data. It never receives another Machine's
  state or control-owned secrets.

## 3. Durable control materialized view

- Apply each authenticated batch atomically and derive Machine identity only
  from its link credential. Reject claimed cross-Machine identities before any
  row changes.
- Store only the approved projection fields and retention windows. Keep a
  per-Machine committed cursor and enough metadata to explain freshness,
  partial resync, and corruption without exposing payloads in audit logs.
- Serve an owner-authenticated bounded initial snapshot directly from control
  SQLite even when workers are offline. Stream committed projection changes
  through one control-origin connection.
- The control Machine's own local state enters the same projection semantics
  without loopback network calls or a separate special wire model.
- Revocation prevents future updates and live actions. Retain or purge the
  last projection only according to the approved explicit policy and show the
  result honestly.

## 4. PWA cutover and UX

- Hydrate the dashboard from the control snapshot first, then apply ordered
  deltas by Machine/cursor. A reload must not wait for every worker.
- Keep one immutable in-memory snapshot per Machine and replace it atomically;
  never merge foreign or out-of-order records.
- Label stale/offline data with last synchronization time. Do not show cached
  Metrics as live movement and do not turn worker Session states into
  `DISCONNECTED` merely because transport is down.
- Enable terminal/input/resize/kill/Continue/Recipe controls only after a live
  authoritative route check. The daemon remains authoritative even if the UI
  mistakenly enables a control.
- History/search may continue as explicit live partial-result queries in this
  milestone unless the approved design includes a separately reviewed,
  privacy-safe Command projection. Never silently display cached Command text
  as complete global history.
- Do not put projection responses in Cache Storage, localStorage, URLs, or
  browser logs. A future optional IndexedDB offline cache is out of scope unless
  separately approved.

## Tests first

- Baseline/adapter: request counters prove the old N+1 behavior, then prove one
  initialization, incremental Event cursors, coalesced refreshes, bounded
  concurrency, visibility wake-up, and no two-second full replay.
- Protocol/HTTP: snapshot, delta, acknowledgement, tombstone, capability,
  bounds, old-worker fallback, fixtures, safe defaults, and generated
  TypeScript where applicable.
- Store migration: upgrade a populated pre-4.D.1 control and worker database
  without reset; projection/outbox cursor atomicity; compaction; tombstones;
  retention; corruption; restart; and rollback preservation.
- Sync: lost acknowledgement, duplicated/reordered batches, control offline,
  worker restart, control restart, full resync, stale cursor, bounded overflow,
  high-frequency Metrics coalescing, and one worker isolated from another.
- Security/privacy: wrong/revoked credential, claimed Machine mismatch,
  oversized payload, cross-Machine overwrite, audit redaction, and scans proving
  no output bytes, log contents, input, environment, path, Command text/cwd,
  subscription secret, Machine credential, or owner cookie enters projection
  rows or browser responses.
- Web: instant cached snapshot, ordered deltas, stale/offline labels, partial
  history, live-action gates, reconnect, logout, and no projection data in
  Cache Storage/localStorage.
- Scale fixture: at least 10 Machines and 1,000 Session summaries with injected
  150 ms worker latency. Assert critical-render operation counts are constant
  with respect to Session count: one control snapshot, one control live stream,
  zero per-Session Event calls, and zero worker calls.
- Playwright against a production build, one control, and two fake workers:
  sync both, reload with one worker offline, render both snapshots immediately,
  receive a delta from the healthy worker, and prove attach/input still routes
  only to the owning live worker. Capture every request and assert no mock data,
  direct worker origin, credential, or N+1 Event fetch.
- Fast tests use no public network, real sleeps, real service manager, or real
  Machine assumptions.

## Real-Machine acceptance

Record literal sanitized evidence in `docs/progress/milestone-4d1.md`:

1. Install the same candidate on the available control and two workers; record
   versions, Machine IDs, OS versions, and Tailscale IPs without credentials.
2. Record request counts and transferred bytes for cold load, warm reload, and
   five idle minutes before and after the change.
3. Start Sessions on both workers, produce Events and Metrics, reload the PWA,
   and confirm the dashboard appears from the control projection before either
   worker is contacted for a live action.
4. Disconnect one worker. Confirm its snapshot is visibly stale/offline while
   the other remains live. Attempted input/kill on the offline worker must fail
   without changing cached state.
5. Reconnect it and verify exactly-once ordered catch-up without duplicate
   Events or Metrics storms. Restart worker and control separately and repeat.
6. Attach to one live FULL Session and inspect/replay one finished Session.
   Confirm bytes still travel from the authoritative worker and are absent from
   the control projection database, logs, audit, browser storage, and cache.

## Done when

The dashboard critical path is independent of worker count and Session count
after synchronization; the two-second full replay loop and per-Session Event
N+1 are gone; a bounded durable control projection makes offline last-known
state immediately visible; every live action and terminal byte remains
worker-authoritative; privacy scans pass; `just check` and production
Playwright pass; measured before/after request counts are recorded; Roadmap
4.D.1 and `docs/progress/milestone-4d1.md` are complete. Send the required
milestone report and stop before 4.E or any broader replication/backup work.
