# MILESTONE 4.D — Native Push Gateway

Goal: an Event on any enrolled Machine produces an encrypted standards-based
Web Push notification from the control `sentinald` to the installed Sentinal
PWA. Sentinal owns notification routing, subscriptions, retry state, and
delivery history; a separately operated ntfy server is no longer required.

This milestone establishes a provider boundary that a future native app can
extend with APNs or FCM. It does not build a native app, bypass mobile operating
system push services, expose the control daemon publicly, add AI-generated
notification text, or copy terminal output to a push provider.

Depends on completed Milestones 4.0, 4.A, 4.B, and the automated portion of
4.C. Reuse 4.C's structured notification presentation, capture checks,
deduplication, persistent delivery journal, safe namespaced Session routes,
browser authentication, production PWA, and retry classifications. Do not
create a second notification daemon.

Before changing code, read `AGENTS.md`, `docs/ARCHITECTURE.md`,
`docs/DEPENDENCIES.md`, `docs/SECURITY.md`, `docs/SETUP.md`,
`docs/progress/milestone-4a.md`, `docs/progress/milestone-4c.md`, the current
notification service, Machine-link routes, service worker, and production web
serving code. Add Milestone 4.D to `docs/ROADMAP.md` and create
`docs/progress/milestone-4d.md` with tasks, proving tests, open questions, and
the exact real-device matrix.

## Required design review — stop before tests or implementation

Present one concrete design covering all of these points:

1. **Ownership and flow.** The control `sentinald` is the only push gateway.
   Workers turn eligible stored Events into bounded structured notification
   intents and forward them over the existing authenticated Machine link.
   Define the exact endpoint/message, replay protection, Machine identity
   binding, offline behavior, ordering, acknowledgement, and bounds. A worker
   must never receive a VAPID private key or another Machine's subscription.
2. **Provider boundary.** Define one small internal delivery-provider interface
   with a Web Push implementation now and explicit APNs/FCM extension points
   later. Do not add placeholder APNs/FCM wire fields, SDKs, credentials, or
   dead implementations. Show how the existing ntfy sender fits as a temporary
   compatibility provider or is disabled without creating duplicate delivery.
3. **Web Push dependency.** Propose the exact maintained Rust crate(s), feature
   flags, license, transitive security surface, and why implementing Web Push
   encryption by hand would be less safe. Add approved dependencies to
   `docs/DEPENDENCIES.md` only after explicit approval. Stop if safe Web Push
   requires `unsafe`, a native library, or an unmaintained dependency.
4. **Key lifecycle.** Specify owner-only VAPID signing-key generation, storage,
   backup, permissions, validation, public-key exposure, rotation, recovery,
   and uninstall behavior. Keys and subscription secrets must never appear in
   URLs, argv, logs, audit entries, browser storage beyond the browser-managed
   Push subscription, or worker configuration.
5. **Subscription contract.** Define authenticated subscribe, list/status,
   refresh, and unsubscribe operations; bounded endpoint/key lengths; device
   labels; expiration; duplicate subscriptions; maximum devices; and ownership
   rules. Prefer additive control-origin HTTP resources over terminal
   WebSocket messages. Show representative JSON and generated TypeScript
   impact if protocol types are necessary.
6. **Endpoint and SSRF policy.** Browser-provided Push endpoints are untrusted.
   Define HTTPS-only validation, accepted provider policy, DNS/IP checks,
   redirect refusal, request limits, timeout, and rejection of loopback,
   link-local, private, Tailnet, credential-bearing, or malformed destinations.
   Tests must not depend on public provider hosts.
7. **Payload and privacy.** Define the encrypted payload schema and strict size
   limit. It may contain only a safe title, bounded structured body, category,
   priority hint, timestamp, and validated internal Machine/Session route.
   Terminal output, Command text, filesystem paths, environment values,
   credentials, pairing data, and arbitrary URLs are forbidden. Explain what
   metadata remains visible to a push service despite payload encryption.
8. **PWA behavior.** Notification permission requires an explicit user gesture.
   Define supported browsers/versions, Home Screen requirements on iOS,
   subscription refresh, service-worker `push` and `notificationclick`
   handling, foreground behavior, logout, owner-password change, and multiple
   devices. Clicking must reuse 4.C's validated internal login return route.
9. **Delivery semantics.** Map Web Push success, timeout, connection failure,
   `404`/`410` expired subscription, `413`, `429`, other `4xx`, and `5xx` into
   the existing bounded retry and journal model. Define concurrency, backoff,
   deduplication, daemon shutdown, restart recovery, and when a subscription is
   disabled. Event and PTY hot paths must never wait for push delivery.
10. **Migration and compatibility.** Existing notification rules and journal
    rows must remain meaningful. Define a no-duplicate transition from
    `notify.toml`, whether ntfy remains an explicitly selected fallback for one
    compatibility window, and how rollback behaves. No installer may silently
    delete `notify.toml` or ntfy history.
11. **Native-app future.** Document which pieces a future APNs/FCM provider can
    reuse—notification intent, policy, routing, journal, device ownership—and
    which credentials and device-token contracts must remain provider-specific.
    Do not claim Web Push subscriptions can be reused as APNs/FCM device tokens.
12. **Platform truth.** State clearly that Sentinal has no public inbound bind,
    but push delivery requires outbound Internet access to browser push
    services. A fully private Tailnet connection cannot reliably wake a
    suspended iPhone. Document expected behavior when Internet or Tailscale is
    unavailable.

Show the complete Event → worker outbox → authenticated control ingress →
policy/dedup → encrypted provider request → journal transition, representative
subscription and intent JSON, storage layout, key permissions, important
errors, and rollback sequence. Then stop for explicit human approval. Do not
install a dependency, generate a real key, register a real subscription, send
external push traffic, or edit production notification credentials before
approval.

If the proposal breaks an existing fixture or cannot remain additive, report
the compatibility break and stop. Keep `PROTOCOL_VERSION = 1` when the approved
contract is additive.

## 1. Control-owned subscription and key store

After approval:

- Generate the VAPID key once on the control Machine with cryptographically
  secure randomness. Store it in an owner-only, non-symlinked bounded file and
  replace it atomically. Refuse unsafe ownership or permissions.
- Persist subscriptions relationally in the control SQLite database with a
  stable subscription ID, owner, bounded optional device label, endpoint,
  browser keys, creation/update/last-success timestamps, and enabled state.
  Encrypt or otherwise protect stored subscription secrets as approved in the
  design; never return private key material to the browser.
- Authenticated owner routes expose only the public VAPID key and secret-free
  subscription status. Subscription mutation requires the production owner
  cookie, exact allowed Origin, CSRF protections already used by control
  mutations, and bounded request bodies.
- A logout removes only the current browser subscription when the approved
  semantics require it; removing one device never disables another. Password
  change, Machine revocation, backup, restore, and uninstall follow the
  approved ownership rules.
- Control startup validates keys and subscriptions independently. One corrupt
  subscription is disabled with a safe journal reason and cannot prevent the
  daemon, PWA, or other devices from working.

## 2. Authenticated worker notification outbox

- Preserve the worker's current Event eligibility, capture-honesty, rule, and
  dedup decisions unless the approved design centralizes a specific decision.
  Never scrape terminal output to produce a notification.
- Store a bounded worker outbox before attempting delivery to the control so a
  temporary control outage does not block or silently lose an eligible intent.
  Define retention, maximum rows/bytes, overflow behavior, and restart replay.
- Authenticate every upload with the existing Machine-link credential. The
  control derives the Machine ID from that credential and rejects a conflicting
  claimed identity before persistence.
- Use idempotency identifiers so retries, reconnects, daemon restarts, and two
  concurrent uploads produce one logical intent. Acknowledgement means the
  control durably accepted the intent, not that a phone displayed it.
- A revoked, disconnected, unpaired, or wrong-control worker cannot enqueue
  into the control. Local Event storage continues to work and no provider
  credential is exposed in the error.
- Bound batches, intent age, title/body/category fields, clock skew handling,
  request frequency, and concurrent worker ingress. Do not put raw Event JSON
  in security audit logs.

## 3. Web Push delivery

- Convert the approved notification intent into the existing safe 4.C
  presentation and an encrypted, bounded Web Push payload. Revalidate the
  internal target route at the control boundary.
- Send independently to each enabled owner subscription through the provider
  queue. One bad or expired device cannot delay another device or another
  Machine.
- Reuse the persistent delivery journal and retry scheduler, adding only the
  provider/subscription identity and result states required to explain
  delivery. Journal and browser status remain secret-free.
- Disable expired subscriptions deterministically on the approved provider
  response. Transient failures retry with injected clocks and bounded backoff;
  permanent failures do not spin.
- Keep ntfy available only under the approved explicit migration setting.
  Never send the same intent to ntfy and Web Push unless the owner explicitly
  selected multi-provider delivery and the UI makes duplication clear.
- Provider HTTP runs outside PTY, watcher, Event, heartbeat, and web request
  hot paths. Shutdown drains only for a short approved bound and leaves durable
  work for restart.

## 4. PWA permission, subscription, and notification UX

- Settings → Notifications explains platform requirements before requesting
  permission. Permission is requested only from a visible owner gesture; no
  startup prompt or repeated nagging.
- Show unsupported, not installed, permission-default, denied, subscribing,
  active, expired, and error states without claiming delivery to a phone.
- Allow the owner to label and remove the current device and inspect all active
  device labels/last-success times without exposing endpoints or browser keys.
- The service worker displays the approved safe title/body and handles
  notification clicks using only a validated same-origin internal route. It
  focuses an existing Sentinal window when possible or opens the production
  PWA. External URLs and payload-controlled origins are rejected.
- Foreground and background delivery must not duplicate visible notifications.
  Existing 4.C previews remain clearly marked as previews and never create an
  Event or fake a successful delivery.
- Preserve the app-shell cache boundary: push payloads, subscription responses,
  authentication, terminal data, and notification history never enter Cache
  Storage.
- Verify accessibility and layouts at 390×844 and 320×568. The application
  must still work when notification permission is denied.

## 5. Operations and compatibility

- Add safe `sentinal notifications status`, `test`, and device-management
  operations only if they use authenticated local/control paths and never put
  keys or endpoints in argv. A test notification is labelled as a test and
  does not fabricate a stored Session Event.
- Installation generates no key until the control role is explicitly enabled.
  Upgrade preserves keys, subscriptions, rules, journals, and `notify.toml`.
  Uninstall retains them by default; destructive removal requires an exact path
  and explicit confirmation.
- Backup and restore documentation distinguishes the VAPID private key from
  browser subscriptions and explains the effect of losing or rotating it.
- Update `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/SETUP.md`,
  `docs/CLI.md`, `docs/DEPENDENCIES.md`, and notification UI copy. Do not call
  provider acceptance “end-to-end display confirmation”; only the real-device
  run proves appearance.

## Tests first

- Protocol/HTTP contract: representative subscribe, refresh, unsubscribe,
  worker intent, acknowledgement, duplicate, bounds, safe defaults, fixture
  coverage, and generated TypeScript where applicable.
- Key store: generation, restart stability, unsafe mode/owner/symlink refusal,
  corrupt file, atomic replacement, backup/restore, rotation, and redaction.
- Subscription store: create/update/remove, duplicate endpoint, device limits,
  multi-device isolation, malformed keys, oversized fields, and migration from
  the pre-4.D database without reset.
- Security: HTTPS-only endpoint validation; credential-bearing URL, redirect,
  loopback, private, link-local, Tailnet, malformed DNS, and rebinding-safe
  resolution behavior; no public network in fast tests.
- Worker outbox: control offline/restart, ordered replay, idempotent duplicate,
  bounded overflow, stale intent, wrong/revoked Machine credential, and one
  worker isolated from another.
- Provider with an injected fake Web Push transport: exact encrypted request
  metadata and safe decrypted fixture payload; success, timeout, connection
  failure, expired subscription, payload too large, rate limit, permanent 4xx,
  transient 5xx, concurrency, shutdown, and restart recovery without sleeps.
- Privacy scans: no terminal output, Command text, paths, Machine credential,
  VAPID private key, subscription auth key, endpoint, or plaintext payload in
  logs, audit entries, URLs, Cache Storage, worker config, or browser-readable
  status responses.
- Service worker: push display, foreground deduplication, internal click route,
  focus/open behavior, malformed/external route refusal, logout, subscription
  refresh, denied permission, unsupported browser, and no cached payloads.
- Web: device state and removal, accessible permission flow, safe test preview,
  delivery history, offline worker/control states, 390×844 and 320×568.
- Playwright against a production build and two fake workers: install/subscribe,
  automatic stored Event → worker outbox → control journal → fake provider →
  service-worker notification → click/login return → exact namespaced Session.
  Assert no request uses mock data and no worker/provider credential reaches the
  browser.
- Fast tests use no public network, real service manager, real provider, or
  real sleeps. External browser push and phone behavior belong only in the
  explicit ignored/manual acceptance.

## Real-device acceptance

Run against the exact candidate commit and record sanitized literal evidence
in `docs/progress/milestone-4d.md`:

1. Publish/install the same release on the macOS control, macOS worker, and
   Linux worker available to the project. Record OS/browser versions,
   Tailscale versions/IPs, binary versions, and Machine IDs without secrets.
2. Install the production Sentinal PWA from Tailnet HTTPS on the real phone.
   From an explicit gesture, grant notification permission and confirm the
   device becomes active after refresh and daemon restart.
3. On each enrolled worker, produce a real successful, failed, and
   waiting-for-input Event. Confirm exactly one safe notification per approved
   rule and that denial/dedup/retry states are honest.
4. Tap notifications for two Machines with the same local Session number.
   Each must pass through login when necessary and open only its exact
   namespaced Session; finished output is read-only and live FULL capture can
   attach.
5. Take one worker offline, restart the control, temporarily remove outbound
   Internet, reconnect, revoke a Machine, and expire/remove one subscription.
   Confirm durable bounded recovery and isolation.
6. Add a second browser/device when available. Removing one subscription must
   leave the other active. Name unavailable device/platform coverage instead
   of claiming it.
7. Inspect SQLite, configuration, audit, logs, browser storage, Cache Storage,
   and network metadata. Prove forbidden terminal data and credentials did not
   cross boundaries. Commit no real subscription, endpoint, private key,
   Machine credential, screenshot containing secrets, or push payload.

## Done when

The control `sentinald` securely owns Web Push keys and subscriptions; eligible
structured Events from either enrolled Machine reach the installed real-phone
PWA without an ntfy server; taps route to the exact Machine and Session;
delivery and failure states survive restart and remain bounded; current rules,
privacy guarantees, production PWA, and optional ntfy migration remain intact;
`just acceptance` passes; and the available real-device matrix is recorded.
Send the required milestone report and stop before Milestone 5 or any native
APNs/FCM application work.
