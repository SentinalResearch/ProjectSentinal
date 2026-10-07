# Milestone 4.D progress — Native Push Gateway

Status: **BLOCKED — automated implementation complete; real-device acceptance
pending.** The human approved the design on 2026-10-06. Contract, persistence,
authenticated ingress, encryption, provider scheduling, and PWA device
controls are implemented. No real subscription or external push was created
during automated tests.

## Baseline inspected

- Each daemon currently evaluates notification rules and capture evidence from
  stored structured Events. It stores retry state in its own SQLite database
  and sends directly to ntfy with bounded concurrency and retries.
- The existing local journal deliberately stores no terminal output, Command
  text, checkpoint path, prompt, credential, upstream response body, or ntfy
  token. That privacy boundary remains binding.
- Enrolled workers already authenticate to the control with a unique worker
  credential. The control derives and validates the Machine identity for
  heartbeat/refresh requests, and workers already have a no-redirect HTTPS
  client plus a durable `link.json`.
- The production PWA is already served by the control `sentinald` at the
  authenticated Tailnet HTTPS origin. The service worker caches only the
  allowlisted app shell and currently has no `push` or `notificationclick`
  handlers.
- Owner HTTP mutations already require the Secure HttpOnly browser Session
  cookie and exact allowed Origin. Machine tokens are refused for control
  registry mutations.
- Notification delivery and worker-to-control acceptance are different facts.
  Reusing one database row for both would make `accepted by control` look like
  `accepted by a browser push service`, so 4.D uses distinct worker-outbox and
  control-delivery records.

## Proposed design

### 1. Ownership and complete flow

The control-role `sentinald` is the only Web Push gateway. No worker receives a
VAPID private key, Push endpoint, browser key, subscription ID, or another
Machine's delivery state.

```text
stored Event on Machine
  -> capture/rule/default/dedup decision on that Machine
  -> durable worker notification_intent_outbox row
  -> POST /api/v1/link/notification-intents
       Authorization: Bearer <that worker's link credential>
  -> control authenticates credential and derives Machine ID
  -> one transaction stores the idempotent intent and per-device deliveries
  -> acknowledgement means "durably accepted by control"
  -> provider scheduler builds privacy-safe presentation
  -> RFC 8291 payload encryption + RFC 8292 VAPID authorization
  -> pinned, no-redirect HTTPS request to browser push service
  -> one secret-free journal transition per subscription
  -> PWA service worker displays and routes the notification
```

The control Machine's own Events enter the same control acceptance function
directly; they are not sent through an HTTP loopback. This keeps policy,
idempotency, fan-out, and journal behavior identical.

#### Provider selection and no-duplicate transition

Provider mode is owner-controlled and has exactly three values:

- `DISABLED`: decisions are journalled but no external delivery is attempted.
- `WEB_PUSH`: linked workers upload intents to the control; only Web Push is
  used.
- `NTFY_LEGACY`: existing machine-local ntfy behavior remains active.

There is deliberately no `BOTH` mode in 4.D. A single intent therefore cannot
accidentally reach both ntfy and Web Push.

An additive optional `notification_ingress` field on
`LinkHeartbeatResponse` tells a new worker whether the control has selected
`WEB_PUSH`. An absent field means `false`, so a new worker connected to an old
control retains its old ntfy behavior and an old worker ignores the new JSON
field. A worker persists the last accepted policy with its link state:

- `notification_ingress = true`: enqueue/upload to the control; do not use
  local ntfy, including while the control is temporarily offline;
- absent/false: retain `NTFY_LEGACY` only when that Machine has a valid
  `notify.toml`; otherwise journal `NOT_CONFIGURED`.

Switching modes is an explicit Settings action. The first Web Push
subscription does not silently switch providers. `notify.toml` and its journal
are never deleted.

### 2. Worker intent contract, identity, replay, and bounds

The additive authenticated endpoint is:

```http
POST /api/v1/link/notification-intents
Authorization: Bearer <worker credential>
Content-Type: application/json
```

Representative request:

```json
{
  "machine_id": "01JCONTROLLEDWORKER",
  "sent_at": 1791302400000,
  "intents": [
    {
      "intent_id": "ni_6b73df5d0f6e6cd52c6d9bdfc5be0b9db961e2c829e88f0f9d61444e84ebbe23",
      "event_id": 902,
      "session_id": 128,
      "occurred_at": 1791302395000,
      "session_label": "Qwen Training",
      "facts": {
        "kind": "WAITING"
      }
    },
    {
      "intent_id": "ni_658b9b0267bd935864a00cf29fc9a18c06de202d5caa52a61dcacc3b26fd884b",
      "event_id": 903,
      "session_id": 128,
      "occurred_at": 1791302399000,
      "session_label": "Qwen Training",
      "facts": {
        "kind": "RESOURCE_HIGH",
        "resource": "GPU",
        "percent": 96.0
      }
    }
  ]
}
```

`NotificationFacts` is a closed tagged enum containing only safe structured
facts needed by the existing presentation layer:

- `COMMAND_FINISHED { exit_code }`
- `ERROR`
- `WAITING`
- `STALLED { silent_for_ms }`
- `RESOURCE_HIGH { resource, percent }`
- `CHECKPOINT`
- `PROGRESS { current, total, unit }`

It cannot carry an Event message, prompt, output line, Command, checkpoint
path, filesystem path, URL, or environment value. The control derives the
category from `facts`; a worker cannot submit a conflicting category. The
session label is optional, sanitized on both sides, and limited to 48 Unicode
scalar values and 192 UTF-8 bytes. An absent/invalid label becomes
`Session <id>`.

`intent_id` is deterministic SHA-256 over a versioned canonical encoding of
Machine ID, Event ID, and derived category. It is not a credential. The
control has a unique `(machine_id, intent_id)` constraint. Retries, concurrent
uploads, and daemon restarts therefore resolve to one logical intent.

Representative acknowledgement:

```json
{
  "accepted": ["ni_6b73df5d0f6e6cd52c6d9bdfc5be0b9db961e2c829e88f0f9d61444e84ebbe23"],
  "duplicates": ["ni_658b9b0267bd935864a00cf29fc9a18c06de202d5caa52a61dcacc3b26fd884b"],
  "rejected": []
}
```

An acknowledgement is returned only after the control transaction commits.
Both `accepted` and `duplicates` let the worker delete the matching outbox
row. A bounded rejected item contains only `intent_id` and a stable code such
as `TOO_OLD`, `INVALID_FACTS`, or `UNKNOWN_SESSION`; it never echoes the bad
payload. A revoked/wrong credential rejects the whole request before decoding
intent data into the store.

Ingress limits are 64 KiB per request, 32 intents per batch, 2 KiB encoded per
intent, 32 concurrent ingress requests globally, and 10 requests/second per
Machine with a bounded burst of 20. Intent age is at most seven days and
future clock skew at most five minutes. The control sorts accepted intents by
`occurred_at`, Event ID, then intent ID for journal display. Provider delivery
is concurrent and does not promise phone-display ordering.

The worker outbox is SQLite-backed, capped at 256 pending rows and 512 KiB of
safe encoded intent data. It retries after 5 s, 30 s, 2 min, then every 5 min,
with injected clocks and a seven-day terminal expiry. Overflow and expiry are
visible secret-free journal results. A control outage or Tailscale outage does
not block Event storage and survives worker restart. Unpaired, disconnected,
revoked, or wrong-control workers cannot upload; they never receive provider
credentials.

The control applies defense-in-depth five-minute deduplication for the same
Machine + Session + category, except `COMMAND_FINISHED`. Even a duplicate or
deduplicated intent is durably recorded before acknowledgement. If no active
subscription exists, the intent becomes `NOT_CONFIGURED`; it is not delivered
retroactively to devices added later.

### 3. Additive protocol and compatibility

New protocol types live only in `crates/protocol` and derive TypeScript:

- `NotificationFacts`
- `NotificationIntent`
- `NotificationIntentBatch`
- `NotificationIntentAcknowledgement`
- `NotificationIntentRejection`
- `PushPublicKey`
- `PushSubscriptionRequest`
- `PushSubscriptionInfo`
- `PushSubscriptionList`
- `PushProviderMode`

`LinkHeartbeatResponse` gains:

```json
{
  "status": "ACTIVE",
  "display_name": "RTX PC",
  "next_heartbeat_ms": 30000,
  "refresh_due": false,
  "notification_ingress": true
}
```

The field is `#[serde(default, skip_serializing_if = "Option::is_none")]`
`Option<bool>`. Legacy fixture JSON remains readable as `None`, old workers
ignore the field, and no existing request enum or Envelope changes. New HTTP
resources get new hand-written fixtures and generated TypeScript. This design
is additive and keeps `PROTOCOL_VERSION = 1`.

No new `MachineCapability` enum variant is proposed: an old control could
reject an unknown enum in a new worker heartbeat. The optional response field
is the compatible capability negotiation.

### 4. Provider boundary and dependency review

There will be one small internal provider trait because 4.D has two real
implementations during the compatibility window:

```rust
trait DeliveryProvider {
    fn kind(&self) -> ProviderKind;
    fn send(&self, target: DeliveryTarget, payload: SafeNotification)
        -> ProviderFuture;
}
```

`WebPushProvider` is the new implementation. The existing sender becomes
`NtfyProvider` and is used only in explicit `NTFY_LEGACY` mode. There are no
APNs/FCM structs, wire fields, SDKs, credentials, or placeholder
implementations. When ntfy compatibility is removed, the trait must either
have a second real native provider or be replaced by a plain Web Push struct,
preserving the project's “two real implementations” rule.

Proposed dependencies after approval:

```toml
web-push-native = { version = "0.5.0", default-features = false, features = ["vapid"] }
base64ct = { version = "1.8.3", features = ["alloc", "std"] }
```

`web-push-native` 0.5.0 declares Rust 1.87 and `MIT OR Apache-2.0`. It builds
RFC 8030 Web Push requests, RFC 8188 encrypted content, and RFC 8292 VAPID
authorization with pure-Rust cryptography. With only `vapid`, its direct
surface is `aes-gcm`, `ece-native`, `hkdf`, `http`, `jwt-simple` configured
with `pure-rust`, `p256`, `sha2`, and `base64ct`. `base64ct` is a direct
dependency because application code must strictly decode/encode browser and
VAPID URL-safe base64 values. Existing `reqwest` with rustls sends the built
HTTP request; the Web Push crate does not own transport.

The older commonly named `web-push` crate is rejected because its supported
path brings OpenSSL/native-library coupling. Hand-implementing ECDH, HKDF,
AES-GCM record framing, VAPID JWTs, and provider headers would be a larger and
less reviewable cryptographic surface. The proposed crate contains no required
native library or `unsafe` use in Sentinal code. After approval, the exact
resolved dependency tree and licenses will be recorded before implementation;
any native dependency, unexpected `unsafe` requirement, incompatible MSRV, or
unmaintained resolution stops the milestone.

Primary references reviewed:

- <https://docs.rs/web-push-native/0.5.0/web_push_native/>
- <https://github.com/leotaku/web-push-native>
- <https://raw.githubusercontent.com/leotaku/web-push-native/master/web_push/Cargo.toml>

### 5. VAPID key lifecycle

The key exists only on an explicitly configured control Machine:

```text
~/.config/terminal-agent/
  daemon.toml                  0600
  push-vapid.json              0600, regular file, no symlink
~/.local/share/terminal-agent/
  terminal-agent.sqlite3      0600 (subscriptions and journals)
```

The parent directories must be same-UID, non-symlink, and `0700`. The key file
is versioned and bounded to 4 KiB:

```json
{
  "version": 1,
  "key_id": "vpk_2b928e...",
  "private_key": "<32-byte base64url value>",
  "public_key": "<65-byte uncompressed P-256 base64url value>",
  "subject": "https://sentinal-control.example-tailnet.ts.net",
  "created_at": 1791302400000
}
```

Key generation uses cryptographically secure OS randomness through the
approved crypto crate, validates the P-256 pair, writes a new owner-only file,
fsyncs it and its directory, then atomically renames it. The key is generated
once when the control role and Web Push provider are explicitly enabled—not
on a worker install. Startup refuses an unsafe/corrupt key for push delivery
but still starts Sessions, the PWA, and other devices' non-push features.

Only the public key and key ID are returned to an authenticated owner browser.
The private key never enters SQLite, a URL, argv, environment, log, audit row,
worker config, HTTP response, or browser storage. It is included in the
documented encrypted owner backup alongside control config/data. Losing it
makes existing subscriptions unusable and requires explicit re-subscription.

Rotation is an explicit owner operation with a confirmation naming the number
of affected devices. It atomically installs a new key, disables all old-key
subscriptions with reason `KEY_ROTATED`, and requires each browser to
subscribe again. It never attempts to reinterpret an old subscription under a
new VAPID key. Normal binary rollback/reinstall preserves the file. Uninstall
retains config and data by default; destructive removal uses the existing
exact-path confirmation.

### 6. Subscription HTTP contract and ownership

These additive control-origin routes require a valid owner browser Session,
the exact allowed `Origin`, the existing SameSite cookie protections, JSON
content type, and a 16 KiB body limit. Machine tokens and worker/control link
credentials are rejected.

```text
GET    /api/v1/push/public-key
GET    /api/v1/push/subscriptions
POST   /api/v1/push/subscriptions
POST   /api/v1/push/subscriptions/refresh
DELETE /api/v1/push/subscriptions/{subscription_id}
POST   /api/v1/push/test
```

Public-key response:

```json
{
  "key_id": "vpk_2b928e...",
  "public_key": "BBO8...base64url...",
  "subject_origin": "https://sentinal-control.example-tailnet.ts.net"
}
```

Subscribe/refresh request:

```json
{
  "device_label": "Sohith iPhone",
  "endpoint": "https://push.example.invalid/wpush/v2/opaque-capability",
  "expiration_time": null,
  "keys": {
    "p256dh": "<65-byte uncompressed P-256 key encoded base64url>",
    "auth": "<16-byte auth secret encoded base64url>"
  }
}
```

Secret-free list response:

```json
{
  "provider_mode": "WEB_PUSH",
  "devices": [
    {
      "subscription_id": "ps_c1e83d0f5f1e4c66",
      "device_label": "Sohith iPhone",
      "state": "ACTIVE",
      "created_at": 1791302400000,
      "updated_at": 1791302400000,
      "last_success_at": null,
      "expiration_time": null
    }
  ]
}
```

Limits are 16 active devices for the single owner, a 48-scalar/192-byte
device label, a 2,048-byte endpoint, exactly 65 decoded `p256dh` bytes with an
uncompressed P-256 point, and exactly 16 decoded `auth` bytes. Empty labels,
control characters, invalid base64, invalid curve points, and past expiration
are rejected with field-naming errors.

The endpoint's SHA-256 is the database uniqueness key; plaintext endpoints are
never returned. Reposting the same endpoint upserts its rotating browser keys,
label, expiration, and `updated_at` while preserving one subscription ID.
Another endpoint creates another device until the cap. `refresh` has identical
validation and is idempotent. Removing one ID disables only that device.

On Settings load, the PWA calls `pushManager.getSubscription()`. If it exists,
the PWA refreshes that exact browser-managed subscription and receives its
non-secret ID; it does not copy keys or endpoints into local/session storage or
Cache Storage. Logout does not silently unsubscribe, because notifications are
an owner-device preference rather than a 24-hour browser login. The UI offers
an explicit **Disable notifications on this device** action. Password changes
revoke browser Sessions but retain device subscriptions; the owner can inspect
and revoke every labelled device after signing in again.

Subscription endpoints, `auth`, and `p256dh` are stored in the same owner-only
SQLite database as other control secrets. Encrypting them with a key stored
beside that database would not improve the local threat boundary and would add
key-management complexity/dependencies, so the approved protection is OS file
ownership/mode, safe backups, redaction, and never returning the values. A
future OS keychain design would be a separate reviewed milestone.

One malformed stored subscription is disabled with a stable reason and cannot
prevent control startup or delivery to other devices.

### 7. Push endpoint validation and SSRF defense

Provider hostnames are not hard-coded: browser vendors may change Push
service hosts. Instead, every registration and every delivery enforces a
rebinding-safe public-Internet policy:

- absolute `https` URL only, maximum 2,048 bytes, DNS hostname only, explicit
  or implicit port 443 only;
- no username/password, fragment, control character, backslash, malformed
  percent encoding, or IP-literal host; opaque path/query capability data is
  accepted but never logged;
- resolve through an injected resolver and reject the endpoint if any answer
  is loopback, unspecified, multicast, link-local, private, carrier-grade NAT
  (`100.64.0.0/10`), Tailnet, documentation, benchmarking, or other
  non-globally-routable space;
- re-resolve before every attempt, validate every answer again, and construct
  the attempt client with those validated addresses pinned for that hostname;
  TLS SNI/certificate validation still uses the original hostname;
- disable redirects, allow one request only, connect timeout 3 seconds, total
  timeout 5 seconds, response body never read, and request/payload limits
  enforced before encryption/allocation.

Tests inject DNS answers and an HTTP transport; they never resolve or contact a
public provider. A literal loopback fake provider is allowed only behind a
test-only injected transport, never a production flag.

Important errors are bounded and do not echo the endpoint:

```text
422 endpoint: expected an HTTPS DNS name on port 443
422 endpoint: resolved to a non-public address
422 keys.p256dh: expected one uncompressed P-256 public key
409 subscription limit: owner already has 16 active devices
413 notification intent batch exceeds 65536 bytes
403 Machine '...' does not match the authenticated worker credential
403 Machine link is revoked
```

### 8. Encrypted payload and visible metadata

The maximum plaintext JSON is 2,048 UTF-8 bytes and its schema is closed:

```json
{
  "v": 1,
  "title": "Sentinal · RTX PC",
  "body": "Qwen Training — waiting for input",
  "category": "WAITING",
  "priority": "HIGH",
  "at": 1791302395000,
  "route": "/app/sessions/01JCONTROLLEDWORKER%3A128"
}
```

The control builds this from authenticated Machine registry data, a bounded
sanitized Session label, and `NotificationFacts`. It revalidates the exact
same-origin internal route. Terminal output, Command text, prompts, Event
messages, checkpoint/filesystem paths, environment values, credentials,
pairing data, endpoints, arbitrary URLs, and raw Event JSON are impossible in
the payload type. Historical payload plaintext is not stored in the journal.

RFC Web Push encryption hides title, body, category, timestamp, and route from
the push service. The provider can still observe the subscription endpoint,
control's source IP, destination host, request time/frequency, encrypted size,
TTL, and VAPID public identity/audience. The browser/OS necessarily sees the
decrypted notification to display it.

### 9. PWA permission and service-worker behavior

Settings explains requirements and enables **Turn on notifications** only in a
secure production context with Service Worker, Push Manager, and Notification
support. `Notification.requestPermission()` and
`pushManager.subscribe({ userVisibleOnly: true, applicationServerKey })` run
only inside that visible owner click. There is no startup prompt or repeated
nag after denial.

States shown honestly are: unsupported, PWA installation required,
permission-default, denied, subscribing, active, expired, disabled, and error.
The app continues normally in every non-active state.

The service worker adds:

- `push`: require a data payload no larger than 2,048 bytes, parse only the
  closed schema, validate the internal route, and call `showNotification` with
  safe title/body/category tag. It never caches or persists the payload.
- `notificationclick`: close the notification, find/focus an existing
  same-origin Sentinal window and navigate it to the internal route, or use
  `clients.openWindow(route)`. External/absolute/malformed routes go to
  `/app/overview`.
- `pushsubscriptionchange`: attempt refresh with same-origin owner cookie. If
  authentication has expired, notify open clients to show “sign in to renew”;
  it never embeds an owner credential.

Only the system notification is shown. A foreground page may refresh its
journal after a service-worker message but does not create a second toast or
notification. The 4.C preview remains labelled **Preview** and never creates
an Event, an outbox row, or a delivered result.

The first real-device target is Safari on iOS/iPadOS 16.4 or newer with the
Sentinal web app added to the Home Screen; macOS Safari 16 on macOS 13 or newer
and current Chromium/Firefox are feature-detected and tested where available.
Actual coverage is recorded, never inferred. Browser Push subscriptions are
not APNs device tokens.

References reviewed for browser behavior:

- <https://developer.apple.com/documentation/usernotifications/sending-web-push-notifications-in-web-apps-and-browsers>
- <https://developer.mozilla.org/en-US/docs/Web/API/PushManager/subscribe>

### 10. Control delivery, retries, journal, and shutdown

For every durably accepted non-deduplicated intent, the control creates one
delivery row per enabled subscription, unique on `(intent_id,
subscription_id)`. One expired/bad device cannot block another. Limits are 16
fan-out rows per intent, 32 due rows per scan, and four provider requests in
flight globally.

Provider results map as follows:

| Result | State and action |
|---|---|
| `201`, `202`, or other documented success `2xx` | `DELIVERED`; provider accepted, not proof of phone display |
| timeout / DNS / connection | `RETRYING` |
| `404` or `410` | `PERMANENTLY_FAILED`; disable subscription as `EXPIRED` |
| `413` | `PERMANENTLY_FAILED` / `PAYLOAD_TOO_LARGE`; internal bug, do not retry |
| `429` | `RETRYING`; honor numeric `Retry-After` capped at 5 minutes |
| `408`, `425`, or `5xx` | `RETRYING` |
| other `4xx` | `PERMANENTLY_FAILED`; subscription state `ERROR`, not silently deleted |
| redirect | `PERMANENTLY_FAILED`; redirects are never followed |

Retries use the existing injected-clock schedule: immediately, 5 seconds, 30
seconds, and 120 seconds; bounded `Retry-After` may replace one delay. After
four failed attempts the delivery is terminal. State survives control restart.
Shutdown stops claims, allows a two-second drain, then puts unfinished rows
back in `QUEUED` with result `SHUTDOWN`. Provider work never runs in PTY,
watcher, metrics, Event, heartbeat, or HTTP request hot paths.

The browser-visible journal is control-owned and adds provider and secret-free
device identity/label plus result classes `SUBSCRIPTION_EXPIRED` and
`PAYLOAD_TOO_LARGE`. Existing rows remain readable. `DELIVERED` documentation
becomes provider-neutral (“selected provider accepted the request”) without
changing its serialized spelling. Worker acceptance is shown separately as
`CONTROL_ACCEPTED`; it never sets phone delivery to `DELIVERED`.

### 11. Storage layout and migrations

A numbered additive SQLite migration creates:

```text
notification_intent_outbox   worker durable upload queue
notification_intents         control idempotency/policy decisions
push_subscriptions           owner devices and browser-managed secrets
push_deliveries              one provider attempt state per intent/device
notification_provider_state  one explicit provider mode
```

The tables exist in every daemon database, but subscriptions, provider state,
and push deliveries are used only by the control role. The worker outbox stores
only safe intent fields. The control subscription table stores endpoint,
`p256dh`, and `auth` but never exposes them in queries used by the UI. Audit
rows contain only Machine ID, intent count, byte count, result class, and
timestamp—not raw JSON, endpoint, label, title/body, keys, or credentials.

Existing `notification_deliveries`, notification rules, Events, Sessions, ntfy
history, and `notify.toml` are not rewritten or deleted. Migrations run without
a database reset and rollback-compatible old binaries ignore the new tables
and new optional heartbeat response field.

### 12. Migration and rollback sequence

Upgrade sequence:

1. Install the same 4.D-capable release on control and workers. Existing ntfy
   remains active because provider mode does not change automatically.
2. Sign in to the production PWA, create/validate the VAPID key by explicitly
   enabling Web Push, and subscribe at least one device.
3. Send a labelled test push through the provider queue. It creates no stored
   Session Event and is visibly `TEST`.
4. Explicitly choose **Use Sentinal Web Push**. The next heartbeat advertises
   `notification_ingress = true`; workers persist the policy, cease local ntfy
   delivery, and begin uploading their durable outboxes.
5. After verification, the owner may retain `notify.toml` unused for rollback.

Rollback sequence:

1. Choose **Use legacy ntfy** while the new control is running. Heartbeats
   advertise false; workers finish already accepted control rows but create no
   new Web Push intents, preventing duplicates.
2. Wait until every online worker reports the policy or explicitly accept that
   an offline worker will continue its last persisted policy until reconnect.
3. Roll back binaries. New tables/key/subscriptions remain untouched; the old
   release resumes its existing ntfy tables/configuration.

An old worker continues ntfy because it cannot observe the new policy. The UI
must refuse switching to Web Push while any online enrolled worker lacks the
4.D release, unless the owner explicitly excludes that Machine; this avoids
mixed-provider duplicate/absence ambiguity.

### 13. Future native provider boundary

A future APNs or FCM implementation may reuse:

- structured notification intent and capture/rule policy;
- safe presentation and exact Machine/Session route;
- owner/device relationship and secret-free device label;
- idempotency, deduplication, delivery queue, journal, retry scheduling, and
  provider result classes.

It must define separate provider-specific credentials, device token contract,
endpoint policy, payload/encryption rules, result mapping, and platform UI.
Web Push endpoints and browser keys are never treated as APNs/FCM device
tokens. No native-app fields or credentials are added in 4.D.

### 14. Platform and network truth

Sentinal gains no public inbound listener, Funnel, wildcard bind, or cloud
relay. The owner reaches the control PWA/API over authenticated Tailnet HTTPS.
The control nevertheless needs outbound Internet HTTPS to the browser's push
service. Those browser services are what can wake a suspended phone; a private
Tailnet connection alone cannot do that reliably.

- Worker loses Tailscale/control: Event and bounded outbox persist; upload
  resumes after the link returns.
- Control loses outbound Internet: accepted per-device deliveries retry and
  survive restart; no phone wake occurs meanwhile.
- Phone loses Internet: its browser push service queues/drops according to
  provider policy; Sentinal can claim only provider acceptance.
- Phone has Internet but not Tailscale: notification may display, but tapping
  cannot load the private control origin until Tailscale connectivity returns.
- Control is offline: no new provider request is possible even if workers are
  online.

## Implementation tasks and proving tests

1. [x] **Protocol and migration** — contract fixtures, legacy heartbeat decode,
   generated TypeScript, and pre-4.D database migration tests.
2. [x] **VAPID store** — generation/stability/mode/owner/symlink/corruption/
   atomicity/rotation/redaction tests.
3. [x] **Subscription API/store** — authentication, Origin, CSRF, validation,
   duplicate/upsert, limits, multi-device isolation, refresh/removal tests.
4. [x] **Endpoint policy** — injected resolver/transport tests for public HTTPS,
   every private/reserved range, rebinding, redirects, timeout, and bounds.
5. [x] **Worker outbox/control ingress** — offline/restart/replay/order/idempotency/
   credential binding/revocation/overflow/stale/batch/concurrency tests.
6. [x] **Providers and journal** — encrypted known-answer fixture, safe decrypted
   payload, Web Push response matrix, ntfy compatibility selection,
   no-duplicate migration, shutdown/restart tests.
7. [x] **PWA** — user-gesture permission, browser state matrix, refresh/removal,
   service-worker push/click validation, foreground behavior, logout/password,
   Cache Storage privacy, accessibility, 390×844 and 320×568 tests.
8. [x] **Production Playwright** — two fake workers, automatic stored Event through
   outbox/control/fake provider/service worker/click/login/exact composite
   Session, with no mock data or leaked credential.
9. [x] **Operations/docs** — architecture, security, setup, CLI, dependencies,
   backup/restore/rotation/uninstall, and exact acceptance evidence.

## Open approval points

Approval of this proposal specifically approves:

1. adding `web-push-native` 0.5.0 with only `vapid` and direct `base64ct`
   1.8.3, subject to the post-resolution native/unsafe/MSRV/license check;
2. additive HTTP/protocol types and optional heartbeat response field while
   keeping `PROTOCOL_VERSION = 1`;
3. 16 active devices, 2,048-byte plaintext payload, 256-row/512-KiB worker
   outbox, 32-intent/64-KiB ingress batches, and seven-day worker retention;
4. OS owner-only SQLite/file protection for subscription secrets rather than
   co-located application-level encryption;
5. explicit mutually exclusive `WEB_PUSH` versus `NTFY_LEGACY` migration;
6. logout/password changes retaining subscriptions until explicit device
   removal, and key rotation disabling all subscriptions.

## Real-device matrix to fill during acceptance

| Role | Known Machine / address | OS/browser | Tailscale | Candidate release | Machine ID | Result |
|---|---|---|---|---|---|---|
| Control | Mac mini / `100.93.101.56` | macOS 27.0.1 / browser to record | 1.102.4 | pre-candidate `0.0.1` | `01M3YT9V…` | control health OK; two Web Push deliveries accepted; candidate rebuild pending |
| Worker | MacBook / `100.66.29.67` | macOS / to record | online; version to record | registry reports `0.0.1` | `01M40Q6X…` | direct health OK; real worker Event/push pending |
| Worker | Linux / `100.74.239.89` | Pop!_OS / browser N/A | offline on 2026-10-07; version to record | registry reports `0.0.1` | `01M40Q7C…` | direct health timed out; acceptance pending |
| Phone PWA | owner phone | OS/Safari to record | to record | production PWA | subscription ID redacted | one real notification received by owner; click/routing and restart evidence pending |
| Second device | unavailable unless supplied | to record | to record | pending | redacted | unavailable/pending |

The implementation must not claim real delivery, wake behavior, lock-screen
appearance, or second-device isolation until literal sanitized evidence is
recorded here against one candidate commit.

## Acceptance checkpoint — 2026-10-07

- The Tailnet HTTPS control and loopback daemon health resources both returned
  protocol version 1. The MacBook worker was online and its direct Tailnet
  health resource returned version 1. The Linux worker was offline in
  Tailscale status and its health request timed out.
- The control store was in `WEB_PUSH` mode with one active subscription. It
  contained two accepted encrypted deliveries for control Session 18
  (`ERROR` and `STALLED`, one attempt each, bounded request sizes 820 and 836
  bytes). The owner confirmed that a notification appeared on the real phone.
- This is useful pre-candidate evidence, not the final real-device run: the
  implementation is still a dirty working tree based on `2249111`, so no
  exact candidate commit or release parity is claimed. The phone OS/browser,
  notification-click route, and lock-screen appearance remain unrecorded.
- The browser suite now has a compile-time-only `e2e-test-support` provider.
  Default/release builds cannot enable it. The suite gives every fake daemon
  isolated config/data and gives the control an isolated VAPID key path; it
  never reads or writes the owner's real VAPID key.

## Remaining acceptance work

- Commit the reviewed 4.D implementation, build/install that exact candidate
  on the control and both workers, and record release/OS/Tailscale parity.
- Produce success, failure, and waiting Events on the real MacBook and Linux
  workers; confirm one privacy-safe phone notification for each expected rule.
- Record sanitized phone OS/browser, display, click/login, exact namespaced
  Session routing, and read-only/live behavior.
- Exercise offline control/worker recovery and a second device when available.
- Run the final `just acceptance` candidate after the real-device evidence is
  recorded. Automated tests deliberately use no public Push provider.

## Automated implementation evidence — 2026-10-07

- `just check`: passed. This includes formatting, Clippy with warnings denied,
  the Rust workspace, current TypeScript generation, 149 web tests, 20 eval
  tests, and all deployment/package shell tests.
- Native Push Playwright: 6 tests passed against the production web build,
  one control, and two isolated workers. The new flow creates a standards-valid
  synthetic subscription, switches explicitly from ntfy to `WEB_PUSH`, waits
  for both workers to persist ingress policy, produces stored Command Events,
  verifies encrypted-request byte accounting and accepted deliveries, executes
  the production service worker's `push` handler, and proves login returns to
  each exact composite Machine/Session identity. The provider transport and
  DNS resolver are compile-time-only fakes; no public provider is contacted.
- `just acceptance`: passed after the service-worker safety tests were added;
  its final browser summary was `5 passed (31.0s)`.
- Native Push tests cover additive contract validation, migration, VAPID file
  safety, endpoint/DNS rejection, encrypted payload round-trip, subscription
  API authorization/redaction, provider delivery, provider request byte
  accounting, worker acknowledgement journal state, and secret-free control
  delivery history.
- The authenticated owner resource `GET /api/v1/push/deliveries` returns at
  most 1,000 newest provider rows with safe rendered text and bounded result
  metadata. It never returns endpoints or browser encryption keys.
- The classic service worker imports a separately tested safety helper. Push
  payloads over 2,048 bytes, control characters, malformed JSON, and external
  or traversal-like routes are discarded before display or navigation.
- No external provider was contacted, no production VAPID key was generated,
  and no real browser subscription was created by the automated suite.

The approved design listed a `/api/v1/push/test` convenience route. It was not
implemented because the approved closed intent/category contract has no `TEST`
category; reusing another category would make journal data dishonest. Real
acceptance therefore uses stored structured Session Events, which is also the
milestone's primary acceptance requirement.

## Dependency resolution audit

- `web-push-native 0.5.0` resolved with only `vapid`; it is MIT OR Apache-2.0
  and declares Rust 1.87. Its encryption path is `ece-native 0.5.0` (MIT OR
  Apache-2.0), while VAPID signing uses `jwt-simple` and pure-Rust
  `superboring 0.1.17` (ISC).
- The selected direct Web Push/encryption crates contain no `unsafe` blocks in
  their source. No native OpenSSL or platform library is linked by this path.
- `base64ct 1.8.3` is used directly for strict unpadded browser/VAPID key
  encoding. Implementing RFC 8291 encryption or VAPID signing locally was
  rejected as a larger cryptographic risk.
