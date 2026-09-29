# MILESTONE 4.0 — Secure Machine enrollment and pairing

Goal: after a worker has been installed separately, run `sentinal connect` to
enroll it with one owner-controlled local control Machine using a stable
Machine ID, editable display name, one-time code, and unique revocable
credential. The Machine token stays local and no cloud control plane exists.

Depends on Milestone 3, Milestone 1b.D, and Milestone 1c.1. Package download
and installation are already complete before this stage: pairing must never
be hidden inside `install.sh`, reinstall, update, or service startup. This
milestone establishes identity, pairing, naming, credentials, revocation,
and transport policy; it does not yet aggregate every Machine's Sessions.

Target ceremony:

```console
# Control UI: Settings → Machines → Add Machine
# Displays control URL, one-time code, and a command that omits the code.

worker$ sentinal connect https://sentinal-control.<tailnet>.ts.net --name "RTX PC"
Machine ID: 01J...
Pairing code: [hidden input]
Connected as RTX PC
```

## Required design review — stop before tests or implementation

Inspect the current Machine/auth protocol, single-owner password flow,
Tailscale deployment, local package/control role, mock pairing UI, and CORS
behavior. Present one concrete additive proposal covering:

1. When and how the immutable Machine ID is generated, stored owner-only, and
   preserved across rename, reconnect, reinstall, update, and URL changes.
   Define collision handling and recovery from lost/corrupt identity.
2. Display-name validation, defaults, duplicates, rename authority, and why a
   display name is never an authorization identity.
3. How an authenticated owner requests a short-lived, single-use pairing code
   from the control Machine without exposing any Machine token.
4. Exact pairing-code entropy, lifetime, hashing-at-rest, one-use consumption,
   rate limit, and revocation semantics.
5. How `sentinal connect` reads the code from a hidden TTY prompt, plus a
   bounded `--code-stdin` automation path. A pairing code
   must not appear in argv, a URL/query, environment, process listings, shell
   history, logs, exceptions, analytics, or service files.
6. Representative request/response JSON for code creation, connect exchange,
   Machine identity/metadata/capabilities, heartbeat, credential refresh,
   list, rename, disconnect, reconnect, and revoke.
7. Direction of the connection and how the control Machine subsequently
   reaches the worker over Tailscale. Define URL/address changes, heartbeat,
   last-seen, offline state, and replay protection without adding a cloud
   rendezvous.
8. Storage of the owner-controlled Machine registry and unique per-Machine
   credentials. The browser receives only its control-Machine owner Session;
   the Machine token stays local and worker credentials never enter browser
   storage.
9. Interaction with the existing owner password: first bootstrap, already
   configured Machine, password rotation, browser-session expiry, and loss of
   the control browser.
10. HTTPS through Tailscale Serve, allowed origins, WebSocket origin checks,
   Tailnet ACLs, and whether plain HTTP is permitted only in explicit
   development mode.
11. Audit records and secret-redaction rules for every success/failure path.
12. Backward compatibility and migration of the existing single-Machine web
    login, including the control Machine's own registry entry.
13. CLI exit codes and semantics for `connect`, `connect --code-stdin`,
    `connection status`, `disconnect`, and an already-paired worker.

The approved product direction uses the owner-selected local control Machine
as registry/coordinator. It is not a cloud service. Do not revert to storing a
list of worker tokens in the browser. Milestone 4.A will implement Session
aggregation and routing over the approved Machine connections.

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
- Add or preserve one owner-only stable Machine identity. Reinstall/update may
  not silently replace it, and rename may not mutate it.
- Enforce exact allowed origins for HTTP and WebSocket upgrade. An empty list
  remains secure by default; wildcard origins are refused.
- Pairing endpoints are bounded, rate-limited with an injected clock, and
  available only on allowed loopback/Tailscale bindings.
- Implement `sentinal connect CONTROL_URL [--name NAME]` with hidden input and
  `--code-stdin`; reject any `--code VALUE` form so the secret cannot enter
  argv. Add connection status/disconnect commands using Sentinal vocabulary.
- Replace the mock-only pairing dialog with Add Machine: generate the code,
  show its expiry and a code-free connect command, then transition through
  waiting/connected/expired/cancelled states.
- Persist Machine ID, display name, approved endpoint/capabilities, last seen,
  and unique credential in the control registry with owner-only permissions.
- Keep credentials isolated. Revoking Machine A must not log out Machine B,
  delete its local Sessions, or invalidate the control owner's browser Session.

## Tests first

- Contract fixtures and old-version decode coverage.
- Stable ID generation/persistence/collision, rename, reinstall preservation,
  and corrupt/lost identity recovery.
- Code entropy/format, expiry boundaries, one-use race, hash-at-rest, replay,
  wrong code, rate limit, revocation, restart persistence, and fake clock.
- Security: no code/token in query strings, logs, audit payloads, exceptions,
  browser URLs, argv/process listings, environment, snapshots, or rendered
  service/config files; Machine token stays local.
- CLI: hidden prompt echo behavior through an injected terminal, bounded
  stdin, non-TTY refusal, already paired, control unavailable, expiry during
  exchange, retry, disconnect, reconnect, and every documented exit code.
- Origin matrix for HTTP and WebSocket, including deceptive suffixes,
  schemes/ports, `null`, wildcard refusal, and development exception.
- Browser: Add Machine, code-free command copy, cancel/error/retry, reload the
  control-backed registry, rename duplicate display names safely, expire one
  credential, and revoke one Machine without affecting another.
- Playwright against two fake daemons with distinct credentials; no real
  network in the fast suite.
- Ignored integration over Tailscale Serve on available real hosts.

## Done when

An already-installed worker can be explicitly connected, named, renamed,
disconnected, reconnected, and revoked by stable ID; installation never pairs
implicitly; no Machine token or worker credential enters browser storage;
`docs/SETUP.md` and `docs/SECURITY.md` match the approved ceremony; `just
check` and Playwright pass; Roadmap 4.0 is checked; and the report records the
approved design and remaining platform coverage. Commit and stop before
Milestone 4.A.
