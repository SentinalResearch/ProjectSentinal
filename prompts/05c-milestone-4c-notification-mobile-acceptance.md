# MILESTONE 4.C — Real notifications and mobile acceptance

Goal: work on either enrolled Machine automatically produces a useful,
privacy-safe ntfy notification on the owner's real phone. Tapping it opens the
correct Machine and Session in an installable Sentinal PWA served from the
authenticated Tailnet HTTPS control origin.

Depends on completed Milestones 4.0, 4.A, and the implemented portion of 4.B.
This milestone closes the real notification/mobile acceptance gap; it does not
add model-generated summaries, a cloud relay, general public hosting, or a
second notification daemon.

Before changing code, read `AGENTS.md`, `docs/SECURITY.md`, `docs/SETUP.md`,
`docs/AUDIT_TRACKER.md`, `docs/progress/milestone-4a.md`,
`docs/progress/milestone-4b.md`, the current notification implementation, and
the web authentication/routing code. Add Milestone 4.C to `docs/ROADMAP.md`
and create `docs/progress/milestone-4c.md` with tasks, proving tests, real-device
acceptance fields, and open questions.

## Required design review — stop before tests or implementation

Present one concrete design covering:

1. How the production-built `web/dist` app shell is served at the existing
   Tailnet HTTPS control origin without a public bind, a second web server, or
   a new dependency. API and WebSocket routes must remain authenticated and
   must never be cached by the service worker.
2. The canonical notification target URL for a namespaced Machine/Session,
   including URL encoding and behavior for a finished, missing, revoked, or
   offline Machine. A login redirect must return to the validated internal
   route after authentication; never accept an arbitrary external return URL.
3. How a worker obtains the trusted control origin used in an ntfy `Click`
   action. A linked worker may use its persisted verified control URL; the
   control Machine may use `[control].public_url`; an unpaired worker must omit
   the action rather than guess. Terminal output, titles, Commands, and Event
   text must never become URLs.
4. The exact ntfy request for each supported category: safe title, short body,
   tag/emoji, priority, and optional `Click`. OS-controlled colors or layouts
   must not be promised. Show representative requests for completed, failed,
   waiting, stalled, resource-high, and checkpoint notifications.
5. Delivery semantics for success, timeout, DNS/connection failure, `429`,
   other `4xx`, and `5xx`: bounded retries/backoff, deduplication interaction,
   journal status, and daemon shutdown. No retry loop may block Session/Event
   processing. State explicitly whether retry state survives daemon restart.
6. Configuration on the control Machine and every worker: `notify.toml`
   ownership/mode, server/topic/token validation, startup/reload behavior, and
   how status is shown without returning the token to the browser. Installation,
   enrollment, Machine credentials, and notification credentials remain
   independent.
7. The real-device matrix available for this milestone: phone OS/browser,
   ntfy app or web client, macOS control, macOS worker, Linux worker, Tailscale
   versions, and unavailable coverage. Do not claim a platform that was not
   run.
8. How notification previews are generated from structured Event data and
   clearly labelled as previews. Preview/test controls must not fabricate
   stored Events, execute a Command, or masquerade as successful external
   delivery.
9. Compatibility and security impact. Prefer no protocol change. If an
   additive request/status field is necessary, show its JSON, fixtures, safe
   defaults, and generated TypeScript impact. Stop if an existing fixture
   would break or a dependency outside the approved stack is needed.

Then stop for explicit human approval. Do not expose ntfy credentials, publish
the PWA, edit real worker configuration, or send an external notification
before approval.

## 1. Automatic notification pipeline

After approval:

- Preserve the current rule source: `EventKind::notifies_by_default()` plus
  per-Machine defaults and per-Session overrides.
- Exercise the real Event pipeline. A notification must result from a stored
  structured Event produced by Command tracking/watchers; production code may
  not scrape terminal text separately or rely on a manual UI trigger.
- Keep capture honesty: a Session may notify only from evidence its
  `CaptureLevel` can observe.
- Retain five-minute deduplication for the same Machine + Session + category,
  except the approved `COMMAND_FINISHED` behavior. A failure to deliver must
  follow the approved retry/dedup semantics rather than silently suppressing
  all attempts for five minutes.
- Delivery runs off the PTY, shell-hook, metrics, and Event hot paths. Bound
  request body/header sizes, timeouts, retries, concurrent deliveries, and the
  in-memory/persisted journal.
- Record only structured metadata: Machine ID, Session ID, Event category,
  attempt/result class, timestamp, and byte count. Never record the ntfy token,
  topic credential, raw response body, raw terminal output, or Command text.
- Keep notification text derived from structured Event fields. Error and
  warning notifications say what kind of Event occurred without copying the
  matched output line.

## 2. Notification appearance and navigation

- Define stable presentation for completed, failed, waiting, stalled,
  resource-high, checkpoint, and progress notifications. Keep title/body short
  enough for a phone lock screen and include the Machine plus a bounded safe
  Session label.
- Use only ntfy-supported metadata already available through HTTP. Do not add a
  mobile SDK merely to control appearance.
- Tapping an actionable notification opens the production control origin and
  navigates to the exact namespaced Session. The control authenticates the
  owner, validates the internal return path, resolves the Machine, and then
  shows live attach or retained read-only output according to Session state.
- If the Machine is offline/revoked or the Session is missing, retain the
  notification context and show an honest recovery message; never route to a
  same-numbered Session on another Machine.
- Add accessible in-app notification previews and delivery history showing
  Machine, Session, category, time, and delivered/failed state. The preview
  must visually say that the final lock-screen appearance is controlled by the
  phone/ntfy client.
- Use `aria-live` for delivery/configuration errors, meet existing focus and
  contrast conventions, and verify narrow layouts at 390×844 and 320×568.

## 3. Production PWA delivery

- Build the Vite app reproducibly and serve immutable hashed assets plus the
  SPA entry point from the existing control process/origin approved in the
  design review.
- Do not expose arbitrary files or follow symlinks. Bound file sizes and set
  explicit content types, cache policy, CSP, frame policy, and nosniff headers.
- API, WebSocket, output replay, pairing, and authentication paths take
  precedence over SPA fallback and are never cached as app-shell assets.
- The service worker caches only versioned app-shell resources, replaces old
  caches on upgrade, and offers an understandable refresh path when a newer
  release is available. Logout/auth responses and terminal data never enter
  Cache Storage.
- Installation and upgrade include the built PWA assets atomically with the
  control release. A failed asset update leaves the previous complete shell
  usable. Document rollback and cache recovery.
- Keep Vite as an explicit development command. The real-device acceptance
  must use the production HTTPS build, not `pnpm dev`, `just demo`, fixtures,
  or an HTTP origin.

## 4. Configuration and operator UX

- Validate `notify.toml` owner-only permissions and values with errors naming
  the file/field but never the token. One invalid Machine must not disable
  notifications on another.
- Settings → Notifications shows each Machine as configured, unavailable, or
  invalid; it shows the server origin/topic and only `token configured: yes/no`.
- Rule edits remain Machine-scoped, persist across restart, roll back in the UI
  on refusal, and cannot cross Machine credentials.
- Add exact setup, status, log inspection, safe test, backup, and removal steps
  to `docs/SETUP.md` and `docs/CLI.md`. Do not recommend putting a token in argv
  or shell history.
- A user must be able to distinguish: Event not produced, rule disabled,
  deduplicated, queued/retrying, delivered, and permanently failed—without
  exposing the upstream response body.

## Tests first

- Notification integration with a fake Event source and local fake ntfy HTTP
  server: each supported category, exact safe headers/body, optional Click,
  no Click when unpaired, and no raw output/credentials.
- Retry matrix with injected clock/client: timeout, connection failure, `429`
  with bounded retry policy, permanent `4xx`, transient `5xx`, recovery,
  shutdown, concurrency bound, and dedup interaction. No real sleeps.
- Routing: notification URLs for two Machines that both have Session `1` open
  only the intended composite identity; malformed/external return paths,
  revoked Machines, missing Sessions, and expired login are safe.
- Configuration: missing/valid/invalid files, mode enforcement, token
  redaction, reload/startup behavior, and one bad worker isolated from another.
- Production web serving: SPA route fallback, MIME/cache/security headers,
  traversal/symlink/oversize refusal, API/WS precedence, asset-version upgrade,
  rollback, and no authentication/output data in Cache Storage.
- Web: notification previews/history, per-Machine status, rules rollback,
  offline/missing target UX, accessibility, and 390×844 plus 320×568 layouts.
- Playwright against a production build and two fake workers: automatic Event
  → notification journal → Click URL → login/return → correct Session. Assert
  no request uses mock data and no Machine credential reaches the browser.
- Fast tests use no public network. External ntfy, Tailscale, real service
  managers, and real phone interactions belong only in the explicit manual or
  ignored integration.

## Real-device acceptance

Run against the exact candidate commit and record literal commands/results in
`docs/progress/milestone-4c.md`:

1. Publish and install the same current worker release on the macOS and Linux
   workers. Record binary versions, OS versions, Tailscale IPs, and Machine IDs.
2. Configure private ntfy topics/tokens without printing them. Subscribe on the
   real phone and confirm each Machine reports configured in the PWA.
3. Install/open the production PWA from the Tailnet HTTPS control origin.
   Confirm standalone launch, refresh, offline app-shell behavior, login, and
   update from one build to the next.
4. On each worker, run a successful Command, a failing Command, and a real
   waiting-for-input command. Produce stalled/resource/checkpoint Events where
   the deterministic watcher can do so honestly.
5. Confirm the expected lock-screen appearance, Machine/Session label,
   priority/tag, single-delivery/dedup behavior, and absence of raw terminal
   lines or secrets. Record sanitized screenshots or a textual appearance
   table; never commit topics, tokens, pairing codes, or private output.
6. Tap notifications from both workers. After login if needed, each opens its
   own Session; a finished Session opens retained output read-only, while a live
   FULL Session can attach. Same-numbered Sessions never cross.
7. Disconnect, revoke, and take one worker offline. Notifications and actions
   for the other worker continue; old actions show honest unavailable/revoked
   states.
8. Inspect notification journal/audit/browser storage/cache and worker config.
   Prove no ntfy token, Machine token, link credential, terminal output, or
   Command text crossed the wrong boundary.

## Done when

Automatic notifications from two real enrolled Machines reach the real phone
with the approved safe appearance; notification taps route through login to the
correct namespaced Session; the production PWA is installed from the Tailnet
HTTPS control origin; failure/retry/dedup states are visible and bounded;
`just acceptance` passes; Roadmap 4.A, 4.B, and 4.C real-device boxes and the
progress reports contain the literal candidate evidence. Send the required
milestone report and stop before Milestone 5.
