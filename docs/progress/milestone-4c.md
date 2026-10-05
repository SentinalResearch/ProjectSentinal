# Milestone 4.C progress — Real notifications and mobile acceptance

Status: **IMPLEMENTED — automated acceptance passes; real-device acceptance
pending.** The human approved this design on 2026-10-05. No real ntfy
credential was committed or exposed and no external notification was sent by
the automated suite.

## Baseline inspected before implementation

- `NotificationService` consumes stored structured Events and checks
  `CaptureLevel::supports_evidence`, but it performs the ntfy request inline in
  the Event receiver, has no explicit timeout or retry policy, and can lag the
  broadcast stream while the network is slow.
- Five-minute deduplication and the 100-entry journal exist only in memory.
  Deduplication is reserved before delivery, so one failed request suppresses
  later Events in that category without a visible retry.
- The current label falls back from Session title to exact Command text, and a
  checkpoint notification includes its filesystem path. Neither is acceptable
  for a phone lock screen or delivery metadata.
- `notify.toml` syntax is checked, but file ownership, mode, symlink safety,
  bounded token fields, reload state, and invalid-versus-missing UI state are
  not enforced.
- The app has a manifest and a minimal service worker, but production
  `web/dist` is not installed or served. The only complete browser path is the
  Vite development server.
- The Tailnet control origin already reaches two loopback listeners in the
  same `sentinald`: `/api` goes to the authenticated daemon API on port 7420,
  while `/` goes to the package listener on port 7422. The package listener is
  therefore the smallest existing place to serve the public app shell; it is
  not a second daemon or public bind.
- The web client currently stores a 24-hour owner bearer in `localStorage` and
  places it in WebSocket query strings. Shipping that behavior in the
  production PWA would retain audit finding F3, so production browser transport
  must move to an HttpOnly cookie in this slice. CLI and worker bearer headers
  remain unchanged.
- The web route `/app/sessions/:id` already uses a composite
  `<machine-id>:<daemon-session-id>` identity. It does not yet preserve that
  route through login or render a stable unavailable view when its Machine is
  missing, offline, or revoked.

## Proposed design

### 1. One production Tailnet origin and one existing daemon

The existing package listener in the control `sentinald` will also serve a
manifest-whitelisted production app shell. Tailscale Serve remains the only
HTTPS edge:

```text
https://sentinal-control.<tailnet>.ts.net/
  /api/*                         -> 127.0.0.1:7420 (existing authenticated API/WS)
  /install.sh                    -> 127.0.0.1:7422 (existing package route)
  /packages/v1/*                -> 127.0.0.1:7422 (existing package routes)
  /, /app/*, /assets/*, icons   -> 127.0.0.1:7422 (new static app-shell routes)
```

Both listeners remain loopback-only. There is no Funnel, wildcard bind,
ordinary-LAN bind, public host, second process, or new dependency. The shell
itself must be fetchable before login so it can render the login page; every
API, WebSocket, replay, registry, pairing, and mutation route remains
authenticated by `sentinald`.

`pnpm -C web build` produces hashed assets. A build step creates a bounded
`web-assets.json` allowlist containing release version, relative path, byte
length, SHA-256, and explicit MIME type. Limits are 1 MiB for `index.html`,
8 MiB for one asset, 32 MiB for the whole shell, and 1,024 files. The server
opens only exact manifest entries beneath an owner-controlled real directory,
refuses symlinks/non-regular files/wrong ownership/length or digest mismatch,
and never joins an arbitrary request path to the filesystem.

The worker/control release archive gains `web/` beside `bin/` and `shell/`.
The installer stages an immutable
`<prefix>/share/sentinal/web/releases/<version>/` directory and atomically
replaces a small owner-only `active.json` pointer after the complete shell is
verified. It never uses a `current` symlink. Service failure restores the old
pointer; explicit release rollback selects an already verified directory.
`[packages].web_root` names the exact installed web directory on the control
Machine. Workers install the same release but do not serve the shell unless
the control/package role and `web_root` are enabled.

Static response policy:

| Resource | Cache policy |
|---|---|
| hashed `/assets/*` | `public, max-age=31536000, immutable` |
| `index.html`, SPA fallback, `sw.js`, manifest | `no-cache` |
| API, WebSocket, auth, output, pairing, package errors | `no-store` and never handled by the service worker |

All shell responses set `X-Content-Type-Options: nosniff`,
`X-Frame-Options: DENY`, `Referrer-Policy: no-referrer`, and a CSP equivalent
to `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline';
font-src 'self'; img-src 'self' data:; connect-src 'self' wss:; object-src
'none'; base-uri 'self'; frame-ancestors 'none'; form-action 'self'`. Inline
styles are currently used by React components, which is why `unsafe-inline`
is limited to styles and not scripts.

The service worker receives the generated release ID and exact shell allowlist.
It caches no arbitrary URL. Navigation is network-first with the cached entry
page as an offline shell fallback; API/package/output/auth routes and any
non-GET request are always bypassed. A fully cached new version posts
`UPDATE_READY`; the UI offers **Refresh to update** and activates it only after
the owner chooses, avoiding a reload during terminal input. Failed installation
leaves the old complete cache active, and activation deletes older shell
caches.

### 2. Browser authentication required by production delivery

The existing bearer endpoints remain for compatibility, but the web app moves
to additive `/api/v1/auth/browser/*` endpoints. Setup/login/password-change set
an owner Session cookie and return the existing secret-free
`AuthSessionInfo { expires_at }`; session/logout consume and clear that cookie.

Production uses a `Secure; HttpOnly; SameSite=Strict; Path=/` cookie with no
`Domain`. Development uses a separately named non-Secure cookie only when the
daemon's explicit `development = true` setting is active. Cookie parsing is
bounded and implemented with existing HTTP types. Exact Origin enforcement and
SameSite protect cookie-authenticated mutations; CLI, Machine, link, and
control-to-worker authentication continue to use Authorization headers.

The production web app stores no owner credential in local/session storage and
opens WebSockets without a `token` query. The browser sends the same-origin
cookie during upgrade; the control proxy authenticates it and continues to use
the selected worker's control credential only upstream. Production rejects an
owner token supplied in a WebSocket query. This is an HTTP/authentication
transport addition, not an Envelope wire change.

### 3. Canonical notification action and safe return routing

The canonical action is:

```text
https://sentinal-control.<tailnet>.ts.net/app/sessions/<encoded-composite-id>
```

where the one path component is
`encodeURIComponent(machine_id + ":" + decimal_session_id)`. Example:

```text
https://advanceds-mac-mini-1.tail17dafb.ts.net/app/sessions/01JABC...%3A128
```

Machine ID and numeric Session ID are the only Event-derived values used in a
URL. Titles, Commands, output, prompts, Event messages, paths, and ntfy values
never enter it.

An unauthenticated visit redirects to
`/login?return_to=<percent-encoded-internal-path>`. Validation accepts at most
2,048 bytes and only a normalized route beginning `/app/`; it rejects schemes,
authority-relative `//`, backslashes, encoded separators/traversal, control
characters, fragments, and any external origin. After login, navigation uses
the validated route once and replaces the login history entry. An invalid
return value goes to `/app/overview`.

The exact composite identity is retained throughout resolution:

- a live FULL Session opens the normal attach view;
- a finished FULL Session opens bounded retained output read-only;
- an offline Machine shows the named Machine, Session ID, last-seen time, and
  retry action without substituting another Machine;
- a revoked Machine shows a revoked recovery state and re-enrollment guidance;
- an unknown Machine or missing Session shows a stable not-found context using
  only the parsed IDs;
- two Machines with local Session `1` can never cross-route.

### 4. Trusted control-origin selection

`NotificationService` receives a small read-only origin provider:

1. on the control Machine, use validated `[control].public_url`;
2. on a worker whose persisted link is ACTIVE or temporarily
   CONTROL_UNREACHABLE, use that link's already validated `control_url`;
3. on an unpaired, disconnected, or revoked worker, return `None` and omit the
   ntfy `Click` header.

The origin is queried when the delivery attempt is rendered so enrollment or
revocation does not require a daemon restart. Production accepts only the
existing HTTPS `*.ts.net` control URL policy. Tests may inject a loopback URL
under explicit development mode. No notification config value can override the
control origin.

### 5. Safe appearance and exact ntfy requests

The Machine label and optional Session title are trimmed, have control and
bidirectional-override characters removed, collapse whitespace, and are
bounded to 48 Unicode scalar values. There is no Command fallback: an untitled
Session is `Session <id>`. Header and body construction reject CR/LF. The title
is at most 100 UTF-8 bytes, body 240 bytes, Click 2,048 bytes, token 4 KiB, and
all request headers plus body 8 KiB.

All requests are:

```http
POST https://<configured-ntfy-server>/<configured-topic>
Content-Type: text/plain; charset=utf-8
Authorization: Bearer <optional token>
Title: Sentinal · RTX PC
Tags: <category tag>
Priority: <category priority>
Click: https://sentinal-control.<tailnet>.ts.net/app/sessions/<machine%3Asession>

<bounded body>
```

`Click` and `Authorization` are simply omitted when unavailable. Stable
category presentation is:

| Category | Body example | Tags | Priority |
|---|---|---|---|
| completed | `Qwen Training — completed (exit 0)` | `white_check_mark` | `default` |
| failed | `Qwen Training — failed (exit 3)` | `x` | `high` |
| error | `Qwen Training — error detected` | `rotating_light` | `high` |
| waiting | `Qwen Training — waiting for input` | `hourglass_flowing_sand` | `high` |
| stalled | `Qwen Training — possibly stalled (10 min)` | `warning` | `default` |
| resource-high | `Qwen Training — GPU usage high (96%)` | `fire` | `high` |
| checkpoint | `Qwen Training — checkpoint created` | `floppy_disk` | `low` |
| progress | `Qwen Training — epoch 37/100` | `chart_with_upwards_trend` | `low` |

The `ErrorDetected.message`, `WaitingForInput.prompt`, checkpoint path,
Command text, and raw output are never copied. Colors, grouping, truncation,
sound, and final lock-screen layout are controlled by the phone OS and ntfy
client and are not product guarantees.

### 6. Persistent bounded delivery pipeline

The Event hot path performs only rule/capture evaluation and one short SQLite
transaction. A new numbered migration adds a `notification_deliveries` outbox
referencing the stored Event ID and containing only Machine ID, Session ID,
category, state, attempt count, next-attempt time, result class, timestamps,
and request byte count. It stores no title, body, Command, output, prompt,
checkpoint path, topic, token, response body, or URL. A unique Event/category
key prevents duplicate enqueue.

States are `RULE_DISABLED`, `DEDUPLICATED`, `NOT_CONFIGURED`, `QUEUED`,
`RETRYING`, `DELIVERED`, and `PERMANENTLY_FAILED`. Result classes are bounded
codes such as `ACCEPTED`, `TIMEOUT`, `CONNECTION_FAILED`, `RATE_LIMITED`,
`CLIENT_ERROR`, `SERVER_ERROR`, `QUEUE_FULL`, and `SHUTDOWN`; upstream response
bodies are never read or recorded. The newest 1,000 terminal decision rows are
retained for at most 30 days. Pending rows are never pruned; no more than 256
may be pending, and an overflow becomes a visible `QUEUE_FULL` terminal row.

Deduplication is an atomic query for the same Machine + Session + category in
the previous five minutes. `COMMAND_FINISHED` Events bypass it. One accepted
Event owns one delivery row and all of its retries; later duplicate Events do
not create parallel retry loops. A permanently failed Event consumes only its
normal five-minute window, while its own bounded retries remain visible. A
missing/invalid configuration records `NOT_CONFIGURED` and does not consume a
future dedup window.

A separate scheduler claims at most 32 due rows and executes at most four ntfy
requests concurrently. The `reqwest` client disables redirects, has a 3-second
connect timeout and 5-second total timeout, and never runs in the PTY, watcher,
metrics, or Event receiver task.

Delivery classification:

| Result | Action |
|---|---|
| `2xx` | `DELIVERED`; ntfy accepted the request |
| timeout / DNS / connection | retry |
| `408`, `425`, `429` | retry; honor numeric `Retry-After` up to 5 minutes |
| other `4xx` | permanent failure; no retry |
| `5xx` | retry |
| redirect / malformed response | permanent failure |

There are at most four attempts: immediately, then after 5 seconds, 30
seconds, and 120 seconds; a bounded `Retry-After` may replace the applicable
delay. Tests inject the clock/client and do not sleep. Retry rows survive daemon
restart. On shutdown the scheduler stops claiming work, cancels in-flight
requests after a 2-second grace period, and returns their rows to `QUEUED` with
result class `SHUTDOWN`; restart resumes them without claiming delivery.

The UI can distinguish an absent Event in the Event timeline from every
decision/delivery state above. `DELIVERED` means accepted by ntfy, not proven
displayed by a phone.

### 7. Configuration and reload

Each Machine independently reads
`~/.config/terminal-agent/notify.toml`. The file must be a same-user-owned,
non-symlink regular file with mode `0600` inside an owner-only directory. A
missing file is `UNAVAILABLE`; invalid syntax, ownership, mode, server, topic,
or token is `INVALID`; a valid file is `CONFIGURED`. Errors in logs name the
file and field but never a token value. One worker's state cannot affect any
other worker.

`server` must be HTTPS with no credentials/query/fragment; explicit
development tests may use loopback or a Tailscale HTTP address under the
existing policy. `topic` remains one 1..=256-byte safe path segment. An
optional token is 1..=4096 bytes with no control characters. Installation,
enrollment credentials, Machine credentials, and notification credentials
remain independent; installers never create or copy `notify.toml`.

Startup loads once, then a low-frequency daemon task checks file identity and
metadata every five seconds and calls the same atomic `reload_config`
operation. Tests call that operation directly with an injected clock. A bad
replacement changes status to `INVALID` and pauses new delivery; it never
continues using a token the operator attempted to replace. Removing the file
changes status to `UNAVAILABLE`. The browser sees only state, server origin,
topic, `token_configured`, last successful reload time, and a redacted field
diagnostic. It never receives the token.

### 8. In-app previews and history

No test button fabricates an Event and no preview sends a notification.
Delivery-history rows are backed by real stored Events and outbox decisions.
For each row, the daemon re-runs the same pure presentation function over the
structured Event plus the current bounded Session/Machine labels. The card is
labelled **Preview — phone appearance is controlled by ntfy and your OS** and
shows Machine, Session, category, decision state, attempts, time, safe title,
safe body, tag, priority, and whether an action URL is available.

Configuration/delivery failures use `role="status"` or `aria-live="polite"`;
permanent failures use `role="alert"`. Rule mutations stay Machine-scoped and
optimistically roll back on refusal. Layout tests cover 390×844 and 320×568,
including long labels, invalid configuration, retries, offline targets, and
empty history.

### 9. Additive compatibility proposal

No Session, Event, auth JSON, ClientRequest, or existing request variant
changes. `PROTOCOL_VERSION` remains 1. Existing CLI bearer headers and existing
auth routes remain valid. The new cookie routes are additive HTTP resources.

The notification response needs additive status detail. Add wire enums
`NotificationConfigurationState`, `NotificationCategory`,
`NotificationDeliveryState`, and `NotificationResultClass`. Add optional,
Serde-defaulted fields to `NotificationStatus` and
`NotificationJournalEntry`; retain the existing `configured`, `text`, and
`delivered` fields for old clients. Old JSON still decodes, and old clients
ignore the new properties. `ts-rs` regenerates the corresponding TypeScript.

Representative current/legacy journal JSON remains readable:

```json
{
  "id": 1,
  "machine_id": "rtx-pc",
  "session_id": 128,
  "text": "RTX-PC · Qwen Training: command finished",
  "at": 1790000600000,
  "delivered": true
}
```

New status and journal JSON:

```json
{
  "configured": true,
  "server": "https://ntfy.example",
  "topic": "sentinal-private",
  "token_configured": true,
  "configuration_state": "CONFIGURED",
  "diagnostic": null,
  "reloaded_at": 1790000500000
}
```

```json
{
  "id": 42,
  "machine_id": "01JWORKER",
  "session_id": 128,
  "text": "RTX PC · Qwen Training: waiting for input",
  "at": 1790000600000,
  "delivered": false,
  "event_id": 902,
  "category": "WAITING",
  "state": "RETRYING",
  "attempts": 2,
  "result_class": "TIMEOUT",
  "request_bytes": 187
}
```

All added fields use safe defaults/`Option` and are hand-added to fixtures.
The protocol version changes only if implementation proves an existing v1
fixture cannot remain readable; that would trigger a new stop condition.

No dependency outside the approved stack is proposed. Axum serves files,
Reqwest delivers ntfy, Rusqlite persists the outbox, existing SHA-256 support
verifies assets, and the existing Vite/pnpm toolchain builds the shell.

## Required task and proof checklist (after approval)

- [x] Add protocol status/journal fields and old/new compatibility fixtures —
  `crates/protocol/tests/contract.rs`, `crates/protocol/tests/fixtures/daemon_messages.json`,
  and `just gen-ts --check`.
- [x] Add persistent bounded outbox, deduplication, retry classification, and
  restart recovery — `crates/store/tests/notification_deliveries.rs` and
  `crates/daemon/src/notifications/tests.rs` with a fake clock/client.
- [x] Add safe presentation and trusted control-origin provider — exact
  request tests for every category and link/control/unpaired state in
  `crates/daemon/src/notifications/tests.rs`.
- [x] Enforce `notify.toml` type/owner/mode/value checks and atomic reload —
  `crates/daemon/src/notifications/tests.rs` and config tests.
- [x] Add cookie browser auth and remove production browser storage/query
  credentials — `crates/daemon/tests/api.rs`,
  `web/src/data/api/index.test.ts`, and `web/e2e/enrollment.spec.ts`.
- [x] Add validated login return paths and unavailable composite-Session UX —
  `web/src/routes/guards.test.tsx`, Session route tests, and
  `web/e2e/enrollment.spec.ts` with two workers.
- [x] Build, install, atomically activate, serve, update, and roll back the
  PWA shell — `scripts/tests/package-release-test.sh`, installer tests, and
  `crates/daemon/tests/package_service.rs`.
- [x] Give each packaged service worker a release-specific cache and explicit
  update UX — package shell tests and production-build Playwright.
- [x] Build accessible per-Machine status, structured-Event preview, and
  delivery history at phone widths — web component tests and
  `web/e2e/enrollment.spec.ts`.
- [x] Update Setup, CLI, Security, dependencies, audit tracker, 4.A/4.B/4.C
  progress, and Roadmap — documentation checks.
- [x] Run `just acceptance` and record its exact final summary.
- [ ] Run the real-device matrix below without committing secrets.

## Automated candidate evidence

- Protocol additions remain additive at `PROTOCOL_VERSION = 1`; old optional
  fields receive safe defaults and generated TypeScript is current.
- The production-build Playwright suite passed all five scenarios, including
  two enrolled fake workers, real stored hook Events, local fake-ntfy HTTP
  delivery, distinct composite action URLs, logout/login return routing, and
  browser storage/URL credential assertions: **5 passed (34.9s)** on the final
  tree.
- Web unit/type checks passed **31 files / 144 tests** before the final full
  repository gate.
- Package-release and installer tests prove PWA inclusion, versioned cache
  identity, atomic upgrade, rollback, and service-start rollback.
- The test ntfy server is local and records only assertions. This evidence is
  not a claim that a real phone, Tailnet HTTPS origin, or external ntfy server
  was exercised.
- `just acceptance` passed on 2026-10-05: formatting, all-feature Clippy with
  warnings denied, every workspace test, generated TypeScript check, **31 web
  files / 144 tests**, **20 evaluator tests**, all deployment/package shell
  tests, and production-build Playwright **5 passed**.

## Real-device acceptance record

No Milestone 4.C real-device test has run. Earlier milestones provide only
candidate inventory and must be reconfirmed on the exact release commit:

| Role | Known candidate | Must record on candidate |
|---|---|---|
| Control | arm64 Mac Mini, `100.93.101.56`, Tailnet HTTPS origin already configured | macOS/build, Tailscale version, Machine ID, Sentinal commit/version |
| macOS worker | MacBook Pro, `100.66.29.67`; prior Tailscale CLI `1.102.4` | macOS/build, browser, Tailscale version, Machine ID, installed release |
| Linux worker | x86_64 Pop!_OS 24.04, `100.74.239.89` | OS/kernel, Tailscale version, Machine ID, installed release |
| Phone | not yet supplied | OS/version, browser/version, ntfy native app or web client/version, PWA install mode |

Unavailable platform coverage will be named rather than inferred. Sanitized
screenshots may be stored only after checking them for topic, token, pairing
code, private output, and Command text.

Acceptance fields to append after implementation:

- candidate commit and release version:
- control/worker OS, Tailnet IP, Tailscale version, Machine ID:
- phone OS/browser/ntfy client:
- production HTTPS PWA install/offline/update result:
- successful/failed/waiting/stalled/resource/checkpoint Event commands and IDs:
- category appearance and dedup/retry results:
- Click → login → exact composite Session results:
- offline/revoked/missing recovery results:
- storage/cache/config/audit secret-boundary inspection:
- `just acceptance` final summary: PASS on 2026-10-05; 31 web files / 144
  tests and 5 production-build Playwright scenarios passed. Real-device steps
  remain not run.

## Open questions requiring operator data, not design changes

- Which real phone OS/browser and ntfy client will be used for final
  acceptance? This is recorded at execution time and does not change the
  implementation.
- Which private ntfy server/topic/token will the owner configure? Values are
  deliberately not accepted in this document, argv, repository, or browser.
- Linux and macOS versions/Tailscale versions must be reconfirmed after both
  workers install the same candidate release.

## Approval record

The human explicitly approved the proposal on 2026-10-05. Approval covered
the additive notification fields, persistent outbox, cookie browser
transport, production app-shell packaging/serving, and bounded real-device
procedure. It did not approve public hosting, Funnel, a cloud relay, a mobile
SDK, model-generated notification text, or real credentials in the repository.
