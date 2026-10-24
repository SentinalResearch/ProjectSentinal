# MILESTONE 4.H — Measure Sentinal on real Machines (pre-V1)

Goal: the control continuously measures how Sentinal behaves on the owner's
own Machines, typing delay, terminal open time, page load, link health,
daemon resources, and errors, and shows it on one health page with a report
the owner can download and share. Every later design decision (Link v2,
keepers, UI/UX speed work) is then made from real numbers, with before/after
evidence for each change.

Background:
- `docs/PLAN_R1_TERMINAL_PERFORMANCE.md` section 4 (performance findings).
- `docs/LINK_V2_DESIGN.md` section 10: the success measures this milestone
  must be able to produce *before* Link v2 is built.
- Already done, do not redo:
  - Settings → Diagnostics network test: browser → Machine and control →
    worker round trips, Tailscale path (`GET /api/v1/machines/{id}/ping`);
  - `Server-Timing` on the control's snapshot, registry, and ping responses
    (`crates/daemon/src/control/timing.rs`);
  - the PWA load timeline (`web/src/data/load-timing.ts`);
  - Nagle off on terminal sockets (`crates/daemon/src/nodelay.rs`).

This is a pre-V1 milestone. It is measurement and presentation only: it must
not change how terminals, links, or Sessions behave.

## Owner decisions

| # | Decision | Default |
|---|---|---|
| D1 | Retention of health data on the control | 30 days of one-minute summaries; raw samples 24 h |
| D2 | Machine names in a downloaded report | replaced by "Machine 1, 2, …" (owner can choose real names) |
| D3 | Link round-trip sampling interval per worker | 60 s |
| D4 | Tailscale path sampling interval | 5 min, plus on every ping test |
| D5 | Where the health page lives | Settings → Diagnostics (desktop and phone) |

## Rules for this milestone

- Follow `AGENTS.md`: tests first, `just check` before every commit, files
  around 400 lines, product name "Sentinal", small focused commits (one per
  numbered item).
- **Privacy is the hard rule.** Only numbers, durations, counts, enumerated
  codes, versions, Machine IDs, and timestamps are recorded. Never terminal
  output, keystrokes, Command text, paths, cwd, environment, hostnames
  (except the owner-set display name already in the registry), IPs,
  credentials, or free-form error messages. Error *codes* only, from a
  closed enum. A test sweeps every stored row and every report for
  forbidden content (section T).
- Nothing leaves the Machine except the existing worker → control link and
  a report the owner downloads explicitly. No third-party telemetry, no
  network request to anything outside the Tailnet.
- Owner-only: every new endpoint requires the owner browser session (or the
  worker credential for worker uploads). No new listener; never bind
  `0.0.0.0`.
- Bounded everything: in-memory buffers, request bodies, rows per minute,
  retention, report size. State each bound in code and docs.
- Collection must cost almost nothing: no measurable change in keystroke
  latency, < 1 % CPU on an idle worker, < 2 MB extra daemon memory. Measure
  and report it.
- Additive protocol changes only (new optional fields, new endpoints).
  Mixed versions keep working: an older worker simply has no stats; an
  older PWA ignores new fields.
- No new dependency. Charts reuse the existing dependency-free
  `web/src/routes/app/MetricsChart.tsx`; if it cannot draw what is needed,
  extend it or stop and ask.

## Phase 1 — Collection

### 1.1 Health data model and storage (control)

- Protocol (`crates/protocol`, with fixtures and generated TypeScript):
  - `HealthMetric`: a closed enum of metric names, e.g.
    `TYPING_ECHO_MS`, `TERMINAL_OPEN_MS`, `PAGE_LOAD_STEP_MS` (with a
    `step` label from a closed enum), `LINK_RTT_MS`, `UPLOAD_DELAY_MS`,
    `UPLOAD_FAILURES`, `DAEMON_RSS_BYTES`, `DAEMON_CPU_PERCENT`,
    `LIVE_SESSIONS`, `LOG_DISK_BYTES`, `ROUTE_MS` (with a `route` label
    from a closed enum), `WS_RECONNECTS`, `VIEWER_RESYNCS`, `ERRORS`
    (with a `code` label from a closed enum).
  - `HealthIncident`: `{machine_id, kind, started_at, ended_at, detail_code}`
    with `kind` in `OFFLINE`, `DELAYED`, `RELAYED`, `RESTARTED`,
    `UPDATE_FAILED`, `ERROR_BURST`.
  - `HealthSummary`: per Machine and metric, per minute: count, min, p50,
    p95, max (p50/p95 from a fixed-bucket histogram, buckets documented).
- Store (`crates/store`, migration `011_health.sql`):
  - `health_samples` (raw, 24 h), `health_minutes` (one-minute summaries,
    D1), `health_incidents` (D1). Indexed by machine and time.
  - Rollup and pruning run with the existing metrics retention job.
  - Hard caps: at most 120 raw samples per Machine per metric per minute
    (excess counted in a `dropped` column, never silently lost); total
    `health_*` size reported in `sentinal doctor`'s Storage line.
- Tests: rollup percentiles against a known distribution; retention
  boundaries with an injected clock; caps and `dropped`; migration on an
  existing database.

### 1.2 Worker and control daemon stats

- Each daemon samples itself once a minute: RSS and CPU of `sentinald`
  (from `sysinfo`, already a dependency), live Sessions by capture level,
  log disk use (from the storage module), uptime, restarts since install
  (a counter in the data directory), daemon version, error counts by code
  since the last report.
- Workers add this as an optional `health` field to the heartbeat they
  already send (`LinkHeartbeatRequest`), plus their upload delay and
  failure counts. An older control ignores it.
- The control records its own stats the same way, directly.
- Tests: heartbeat fixture with and without `health`; an older-shape
  heartbeat still accepted; the control stores what the worker reports
  under that worker's Machine ID only (a worker cannot report for another).

### 1.3 Link and Tailscale measurements (control)

- Every D3 interval the control times one identity round trip to each
  online worker (reuse `prove_identity`, as the ping endpoint does) and
  records `LINK_RTT_MS`.
- Every D4 interval, and on every Diagnostics ping, it records each
  worker's Tailscale path (Direct/Relayed + relay region code) from the
  existing `tailnet_dns::real_status` lookup. A change opens or closes a
  `RELAYED` incident.
- Freshness transitions already computed by the control (LIVE → STALE →
  OFFLINE) open and close `DELAYED` and `OFFLINE` incidents. A version
  change after a restart records `RESTARTED`; a failed worker update
  records `UPDATE_FAILED`.
- Control route timings: the `ServerTiming` phases of the snapshot,
  registry, ping, and proxy-setup paths feed `ROUTE_MS`. Add the routed
  terminal setup (identity proof + upstream connect) as a measured route:
  this is the "terminal open" cost Link v2 must beat.
- Tests: incidents open/close correctly with an injected clock and a fake
  status source; no sampling of offline or revoked Machines; sampling
  stops on shutdown.

### 1.4 Browser measurements (PWA)

- **Typing echo:** for each attached terminal, when a printable keystroke
  is sent, start a timer; the next output frame for that terminal within
  1 s stops it. Record the duration (not the key) per Machine. At most one
  timer in flight per terminal; pastes (> 1 byte) are not timed.
- **Terminal open:** from the attach request to the first screen frame
  drawn, per Machine.
- **Page load:** the existing `load-timing` steps, once per page load.
- **Reconnects and resyncs:** WebSocket reconnects per socket kind;
  redraws after lag (the reset frame from P0.2) per Machine.
- **Client errors:** failed actions by error code (closed enum mapped from
  daemon failures), never the message.
- Batching: the PWA buffers samples in memory (max 500) and sends them
  every 30 s and on `pagehide` (`navigator.sendBeacon`) to
  `POST /api/v1/health/samples` (owner cookie, body ≤ 64 KiB, validated
  against the closed enums; unknown names rejected). Nothing is stored in
  browser storage.
- Tests (Vitest): echo timer start/stop/ignore rules with fake timers;
  paste not timed; buffer bound; beacon payload contains only allowed
  fields (sweep test). Daemon tests for the endpoint: auth, size limit,
  enum validation, per-Machine attribution only to Machines the owner has.

## Phase 2 — Health page and report

### 2.1 Health API (control)

- `GET /api/v1/health/summary?range=1h|24h|7d|30d` → per Machine: latest
  stats, p50/p95 per metric over the range, and a bounded series for
  charts (≤ 300 points per metric).
- `GET /api/v1/health/incidents?range=…` → newest first, bounded (200).
- `GET /api/v1/health/report?range=24h|7d&names=anonymous|real` → one JSON
  document: versions, Machines (anonymised per D2), per-metric p50/p95/max,
  incidents, sampling coverage (how many samples, how many dropped), and
  the collection overhead measured in 3.1. `Content-Disposition:
  attachment`, `Cache-Control: no-store`. Size ≤ 1 MiB.
- All owner-only, `Server-Timing` on each. Fixtures for every response.

### 2.2 Settings → Diagnostics becomes the health page

- Top: one card per Machine (control first): status (with the "delayed"
  wording from P0.1), typing delay p50 today, terminal open p50, link round
  trip, Tailscale path, daemon memory, uptime, version. A value outside a
  documented threshold is highlighted with the reason ("Typing delay
  140 ms: far away (US). See Link v2 / predictive echo.").
- Middle: charts per metric (p50 and p95 lines) for the chosen range, one
  Machine or all.
- Incidents timeline: "RTX-PC offline 2 min at 09:14", "relayed through fra
  14:02–14:20", "updated to 0.3.0 at 10:01".
- The existing network test and page-load line stay, below.
- **Download report** button (range + names choice).
- Phone layout at 390 px: cards stacked, one chart at a time, incidents as
  a list.
- Empty states: "No data yet: health data appears after a few minutes of
  use." and "This worker runs an older Sentinal and does not report
  health. Update it from Settings → Machines."
- Tests: Vitest for cards, thresholds, empty states, anonymised report
  download; Playwright against the real control and two workers in the e2e
  chain: generate typing and terminal opens, wait for a flush, see the
  values on the page, download a report and check it parses, contains both
  workers anonymised, and contains no forbidden content. Keep
  `projection-acceptance` last.

### 2.3 `sentinal doctor --report`

- On the control, writes the same report to a file (`--range`, `--names`).
  On a worker, prints that reports come from the control. `--json` already
  exists for doctor; reuse its style.

## Phase 3 — Prove it and set the baseline

1. **Overhead:** measure keystroke echo with collection on and off on this
   Mac (loopback) and one worker; daemon RSS and CPU on an idle worker with
   collection on vs off. Put the numbers in the report section of the
   progress doc. Fail the milestone if collection adds measurable typing
   delay.
2. **Baseline on the owner's Tailnet:** after the owner uses Sentinal
   normally for at least one day, download a 24 h report (anonymised) and
   record in `docs/progress/milestone-4h.md`: typing echo p50/p95 per
   Machine, terminal open p50/p95, page load steps, link RTT, delayed and
   offline incident counts, daemon memory per Machine. This table is the
   "before" for Link v2 (`docs/LINK_V2_DESIGN.md` section 10) and keepers.
3. Update `docs/SECURITY.md` (what is collected, where, retention, who can
   read it), `docs/SETUP.md` (how to read the health page and download a
   report), and `docs/ROADMAP.md` (add the 4.H boxes and tick them).

## Section T — Tests that must exist

- Privacy sweep: a test inserts realistic Sessions with secrets in output,
  commands, cwd, and env, exercises every collector, then scans every
  `health_*` row, every API response, and the report for those strings,
  for IPs, for paths, and for the hostname. Zero hits.
- Every new wire type has a fixture; TypeScript bindings regenerated
  (`just gen-ts --check`).
- No sleeps over 100 ms in fast tests (injected clocks, fake timers).
- Mixed versions: an older worker heartbeat without `health`; an older PWA
  that never posts samples; both work and show the "older Sentinal" empty
  state.
- Do not weaken or delete existing tests.

## Done when

- Phases 1–3 are committed, `just check` and `just test-e2e` pass.
- The health page shows live data from the real control and both test
  workers in e2e, and from the owner's Machines after the update.
- Collection overhead is measured and negligible (numbers in the report).
- `docs/progress/milestone-4h.md` holds the baseline table from the owner's
  Tailnet (or states that it is waiting for the owner's 24 h of use).
- Stop with the report and "Questions for you". Do not tag or push.
