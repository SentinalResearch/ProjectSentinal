# MILESTONE 4.0 — Secure Machine enrollment and pairing

Goal: enroll multiple Sentinal Machines into one local browser without copying
a long-lived Machine token into browser storage or introducing a required
cloud control plane.

Depends on Milestone 3, Milestone 1b.D, and Milestone 1c. This milestone
establishes identity, pairing, credentials, revocation, and transport policy;
it does not yet aggregate every Machine's Sessions into one dashboard.

## Required design review — stop before tests or implementation

Inspect the current Machine/auth protocol, single-owner password flow,
Tailscale deployment, browser credential storage, mock pairing UI, and CORS
behavior. Present one concrete additive proposal covering:

1. Stable Machine identity versus display name and URL changes.
2. How a local owner requests a short-lived, single-use pairing code without
   exposing the Machine token in process arguments, logs, URLs, or the browser.
3. Exact pairing-code entropy, lifetime, hashing-at-rest, one-use consumption,
   rate limit, and revocation semantics.
4. Representative request/response JSON for code creation if remote,
   exchange, Machine metadata/capabilities, credential refresh, list, rename,
   and revoke.
5. Whether code creation is a local-socket request, an authenticated remote
   request, or both. Local-only authority must remain unavailable to an
   ordinary remote browser.
6. How the browser stores a Machine registry and per-Machine browser Session
   credentials. The raw Machine token must never enter browser storage.
7. Interaction with the existing owner password: first bootstrap, already
   configured Machine, password rotation, browser-session expiry, and loss of
   the control browser.
8. HTTPS through Tailscale Serve, allowed origins, WebSocket origin checks,
   Tailnet ACLs, and whether plain HTTP is permitted only in explicit
   development mode.
9. Audit records and secret-redaction rules for every success/failure path.
10. Backward compatibility and migration of the existing single-Machine web
    login.

Prefer direct browser-to-daemon connections and local-only registry state, as
documented in the architecture. If a central coordinator is proposed, explain
why direct aggregation is insufficient and stop for explicit scope approval.

Show the pairing ceremony as a sequence diagram and include representative
JSON plus storage records. Then stop and ask the human to approve it. Do not
edit protocol fixtures or production code until approval is explicit.

If additive, keep `PROTOCOL_VERSION = 1`. If an existing fixture must break,
report it and stop.

## Implementation after approval

- Add approved types only to `crates/protocol`; hand-update fixtures and
  generated TypeScript with every-kind coverage.
- Generate, store, compare, consume, expire, and revoke codes according to the
  approved design. Never store a recoverable pairing code.
- Enforce exact allowed origins for HTTP and WebSocket upgrade. An empty list
  remains secure by default; wildcard origins are refused.
- Pairing endpoints are bounded, rate-limited with an injected clock, and
  available only on allowed loopback/Tailscale bindings.
- Add CLI commands using Sentinal vocabulary and document every exit code.
- Replace the mock-only pairing dialog with the real flow, explicit Machine
  URL validation, recoverable errors, and credential removal.
- Keep each Machine's credentials isolated. Revoking Machine A must not log
  out Machine B.

## Tests first

- Contract fixtures and old-version decode coverage.
- Code entropy/format, expiry boundaries, one-use race, hash-at-rest, replay,
  wrong code, rate limit, revocation, restart persistence, and fake clock.
- Security: no code/token in query strings, logs, audit payloads, exceptions,
  browser URLs, snapshots, or rendered service/config files.
- Origin matrix for HTTP and WebSocket, including deceptive suffixes,
  schemes/ports, `null`, wildcard refusal, and development exception.
- Browser: add two Machines, cancel/error/retry, reload registry, expire one
  credential, revoke one Machine without affecting the other.
- Playwright against two fake daemons with distinct credentials; no real
  network in the fast suite.
- Ignored integration over Tailscale Serve on available real hosts.

## Done when

Two Machines can be securely enrolled and independently revoked, no long-lived
Machine token enters browser storage, `docs/SETUP.md` and `docs/SECURITY.md`
match the approved ceremony, `just check` and Playwright pass, Roadmap 4.0 is
checked, and the report records the approved design and remaining platform
coverage. Commit and stop before Milestone 4.A.
