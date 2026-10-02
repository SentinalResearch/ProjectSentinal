# Milestone 4.0 progress — Secure Machine enrollment

Status: **design approved by the owner 2026-10-02** as written, including
`sha2` + `hmac` and `reqwest` (rustls, no default features), the control
daemon binding to loopback behind Tailscale Serve, and the Machine ID slug
migration. **Implemented 2026-10-02** (see Implementation record below).

## What exists today (inspected)

| Area | Current behaviour | Consequence for 4.0 |
|---|---|---|
| Machine ID | `MachineId(slug(hostname))` built at every start (`entry.rs`) | Not stable: a hostname change renames the ID; two hosts named `pc` collide |
| Owner auth | One Argon2id password per daemon, 24 h in-memory browser sessions, Machine token bootstraps setup | Reusable as the control owner's identity; no Machine-to-Machine credential exists |
| Control role | `run-control.sh` proxies Vite `/api` to **one worker**; the browser logs into the worker's password | No registry; the "control" holds no state |
| Control daemon | Already runs `sentinald` for the 1c.1 package role behind Tailscale Serve (`https://<name>.<tailnet>.ts.net`) | Natural home for the registry and pairing endpoints |
| Origins | No `Origin` check on HTTP or WebSocket upgrade; bearer-only auth | Must be added before a browser-reachable pairing API exists |
| Pairing UI | Mock-only 3-step dialog (`PairMachineDialog`), `pairMachine(name)` in the mock adapter | Replace with the real Add Machine flow |
| Crypto/HTTP | SHA-256 only via shell tools in packaging; no HTTPS client in Rust; `stty -echo` already used by the CLI | Needs an approved hashing crate and an HTTPS client (see Decisions) |

## Roles and trust boundary

```
 owner browser ──HTTPS (Tailscale Serve)──► CONTROL sentinald ◄──HTTPS (Serve)── worker sentinald
   owner Session only                       registry + codes           (pairing, heartbeat)
                                                  │
                                                  └── HTTP over WireGuard ──► worker :7420   (4.A routing;
                                                      to the reported Tailscale endpoint      control credential)
```

- The control Machine is owner-selected and local. There is no cloud service
  and no rendezvous outside the tailnet.
- The Machine token never leaves the Machine that created it. A worker
  credential never reaches a browser.
- A daemon is the control only when `[control]` is enabled in its
  `daemon.toml`. Every other daemon answers the registry and pairing routes
  with `404 NOT_CONTROL`.

## Pairing ceremony

```mermaid
sequenceDiagram
    autonumber
    actor Owner
    participant B as Browser (control UI)
    participant C as Control sentinald
    participant CLI as worker: sentinal connect
    participant W as Worker sentinald

    Owner->>B: Settings → Machines → Add Machine
    B->>C: POST /api/v1/pairing/codes (owner Session, Origin checked)
    C->>C: code = 50 random bits; store HMAC(code), expiry +10 min
    C-->>B: {code_id, code, expires_at, connect_command (no code)}
    B-->>Owner: shows code + countdown + copyable command
    Owner->>CLI: sentinal connect https://control.ts.net --name "RTX PC"
    CLI->>W: local socket GET_IDENTITY (peer UID checked)
    W-->>CLI: {machine_id, link: null}
    CLI-->>Owner: "Machine ID: 01J…" then hidden prompt
    Owner->>CLI: types code (echo off)
    CLI->>W: local socket CONNECT_CONTROL {control_url, code, display_name}
    W->>W: generate control_credential (256-bit)
    W->>C: POST /api/v1/link/exchange over HTTPS (code in body only)
    C->>C: verify HMAC, unexpired, unused, attempts < 5 → consume atomically
    C->>C: registry upsert by machine_id; worker_credential (256-bit)
    C-->>W: {link_id, worker_credential, display_name, heartbeat_interval_ms}
    W->>W: store worker_credential + SHA-256(control_credential), 0600
    W-->>CLI: CONNECTED {display_name}
    CLI-->>Owner: "Connected as RTX PC"
    loop every 30 s
        W->>C: POST /api/v1/link/heartbeat (Bearer worker_credential, sequence+1)
        C-->>W: {display_name, status: ACTIVE}
    end
    B->>C: GET /api/v1/pairing/codes/{code_id} (poll 2 s) → CONNECTED, machine_id
```

## Decisions (numbered as in the milestone prompt)

### 1. Immutable Machine ID

- **Format.** A ULID: 26 Crockford base32 characters (48-bit milliseconds,
  then 80 random bits from `/dev/urandom`), for example
  `01J9Z3K8V6Q2T7M4X5C0RNB8DA`. It sorts by creation time, and two random
  IDs colliding is negligible (80 bits within the same millisecond).
- **When it is created.** On the first daemon start that finds no identity
  file. The installer, update, and reinstall never create or touch it.
- **Where it lives.** `~/.config/terminal-agent/machine-id.json`, mode `0600`,
  written with `create_new` plus fsync and rename. It sits beside `token` and
  `auth.json` in the existing `0700` config directory, so `uninstall` without
  `--delete-config` and every update keep it.

  ```json
  {"version":1,"machine_id":"01J9Z3K8V6Q2T7M4X5C0RNB8DA","created_at":1790900000000,"legacy_machine_id":"advanceds-mac-mini-1"}
  ```

- **What never changes it.** Renames change only the display name. A
  reconnect reuses the ID. A changed control URL or Tailscale address is
  metadata.
- **Corrupt file** (bad JSON, wrong version, ID not a valid ULID, wrong
  owner or mode): the daemon still serves local Sessions, refuses every link
  operation with `IDENTITY_CORRUPT`, and never overwrites the file.
  `sentinal doctor` names the problem. Recovery is explicit:
  `sentinal identity reset` (with a confirmation prompt) moves the bad file
  aside to `machine-id.json.corrupt-<ts>`, creates a new ID, and drops any
  local link. The owner then revokes the old registry entry, which simply
  shows as offline.
- **Lost file** (for example a deleted config directory): a new ID is
  created. To the control this is a new Machine; the old entry goes offline
  and is revoked by the owner.
- **Collisions in the registry.** `machine_id` is the registry primary key.
  An exchange for an ID already in the registry is treated as a reconnect of
  that Machine. That is safe because only the owner can issue the one-time
  code. The old link's credentials are revoked in the same transaction, and
  the event is audited as `LINK_REPLACED`.
- **Migration of the hostname slug.** Store migration 4 rewrites
  `machine_id` in the `machines` and `sessions` rows from the legacy slug to
  the new ID in one transaction, and records the slug as
  `legacy_machine_id`. A single-Machine database has only that one ID.

### 2. Display name

- **Rules.** Trimmed, 1–64 Unicode scalar values, no control characters
  (C0, C1, DEL) and no bidirectional overrides (U+202A–202E, U+2066–2069).
- **Default.** The hostname, cut to the first 64 characters.
- **Duplicates are allowed.** The UI shows the first 6 ID characters next to
  any name that appears more than once, for example `RTX PC · 01J9Z3`.
- **Who sets it.** The worker proposes a name once, at exchange (`--name`,
  otherwise its hostname). After that the control registry is the only
  source of truth and only the owner renames, through the UI. Heartbeat
  responses carry the current name so the worker can show it.
- **Never an authorization identity.** Every lookup, route, credential,
  audit record, and revocation uses `machine_id`. A name can collide, change,
  and contain anything a person types.

### 3. Requesting a code without exposing a Machine token

- `POST /api/v1/pairing/codes` on the control requires a valid **owner
  browser Session**. The Machine token is rejected on this route, so a local
  script holding the token cannot mint codes by mistake.
- The request must also carry an allowed `Origin` (see 10).
- The code is returned once, in the response body. It is never part of a
  URL, never written to browser storage, and held in React state only until
  the dialog closes.

### 4. Pairing-code security

- **Format.** 10 characters from the 32-symbol Crockford alphabet (no `I`,
  `L`, `O`, `U`), shown as `7KQ4M-2XZ9C`: 50 bits from `/dev/urandom`.
  Entry is case-insensitive and ignores `-` and spaces; `O`→`0` and `I`/`L`→`1`.
- **Lifetime.** 10 minutes. A code is valid while `now < expires_at`; at
  exactly `expires_at` it has expired.
- **At rest.** Only `HMAC-SHA256(pairing_key, normalized_code)` is stored.
  `pairing_key` is a separate 256-bit secret in `auth.json` (mode `0600`), so
  a leaked SQLite file alone cannot be brute-forced offline. The code is
  never recoverable.
- **Exactly one use.** The code is consumed with one SQL statement,
  `UPDATE … SET consumed_at=?, machine_id=? WHERE code_hmac=? AND consumed_at IS NULL AND cancelled_at IS NULL AND expires_at > ? AND failed_attempts < 5`,
  in the same transaction as the registry write. When two exchanges race,
  exactly one gets the row.
- **Rate limits** (injected clock):
  - A wrong code cannot be tied to a particular code, so it counts against a
    control-wide budget: **20 failures per 10 minutes**. When the budget is
    used up, exchange returns `429 PAIRING_LOCKED` until the window ends and
    every outstanding code is cancelled.
  - With at most 5 outstanding codes, 20 guesses succeed with probability
    about 9·10⁻¹⁴.
  - Creating codes is limited to **10 per 10 minutes**, with at most **5
    outstanding**.
- **Cancellation.** `DELETE /api/v1/pairing/codes/{code_id}` cancels one
  code. Changing the owner password cancels all of them. Codes in SQLite
  survive a daemon restart and keep their original expiry.

### 5. `sentinal connect` input

- **Interactive.** The code is read from `/dev/tty` with
  `stty -echo -icanon` (the same pattern as `client.rs`), and the previous
  terminal mode is restored on drop, on Ctrl-C (a `SIGINT` guard), and on
  error. At most 64 bytes are read.
- **Automation.** `--code-stdin` reads one line of at most 64 bytes from
  stdin, refuses more, and never echoes it.
- **Refusals.**
  - Without `--code-stdin`, a non-TTY stdin fails with exit 2:
    `pairing code needs a terminal or --code-stdin`.
  - Every form of `--code`, `--code=`, `--pairing-code`, or `-c` fails with
    exit 2. The message never repeats the argument value.
  - The code is never accepted in argv, in an environment variable such as
    `SENTINAL_PAIRING_CODE`, or in a URL query or fragment.
- **Where the code goes.** The CLI wraps the code in a `Secret` type whose
  Debug and Display print `[redacted]`, sends it to the local daemon over the
  `0600` peer-UID-checked socket, and drops it. The daemon puts it in one
  HTTPS request body. No log, `tracing` field, audit record, error, or panic
  message contains it, and tests assert this for every failure path.

### 6. Wire format (representative JSON)

All new types are additive. `PROTOCOL_VERSION` stays `1`.

**Owner → control** (owner Session bearer, Origin checked)

```jsonc
// POST /api/v1/pairing/codes   body: {}
{"code_id":"pc_01J9Z4A7","code":"7KQ4M-2XZ9C","expires_at":1790900600000,
 "connect_command":"sentinal connect https://sentinal-control.example.ts.net"}

// GET /api/v1/pairing/codes/pc_01J9Z4A7
{"code_id":"pc_01J9Z4A7","state":"CONNECTED","expires_at":1790900600000,"machine_id":"01J9Z3K8V6Q2T7M4X5C0RNB8DA"}
// state: WAITING | CONNECTED | EXPIRED | CANCELLED   (the code is never returned again)

// DELETE /api/v1/pairing/codes/pc_01J9Z4A7  → 204

// GET /api/v1/machines
{"machines":[
  {"machine_id":"01J9Y0CTRL0000000000000000","display_name":"Mac mini","role":"CONTROL",
   "status":"ONLINE","last_seen":1790900500000,"endpoint":null,"credential":null},
  {"machine_id":"01J9Z3K8V6Q2T7M4X5C0RNB8DA","display_name":"RTX PC","role":"WORKER",
   "status":"ONLINE","last_seen":1790900590000,
   "endpoint":{"address":"100.74.239.89","port":7420,"dns_name":"pop-os.example.ts.net"},
   "os":"LINUX","daemon_version":"0.0.1","capabilities":["SESSIONS","EVENTS","METRICS","ADOPT"],
   "credential":{"link_id":"lk_01J9Z4B2","created_at":1790900540000,"expires_at":1798676540000,"last_used_at":1790900590000}}
]}
// status: ONLINE | OFFLINE | DISCONNECTED | REVOKED     credential never contains a secret

// PATCH /api/v1/machines/01J9Z3K8V6Q2T7M4X5C0RNB8DA   {"display_name":"RTX 4090 box"}  → updated entry
// DELETE /api/v1/machines/01J9Z3K8V6Q2T7M4X5C0RNB8DA  → 204 (revoke; the entry stays as REVOKED for history)
```

**Worker → control** (HTTPS through Tailscale Serve; no browser Origin)

```jsonc
// POST /api/v1/link/exchange
{"pairing_code":"7KQ4M-2XZ9C",
 "machine":{"machine_id":"01J9Z3K8V6Q2T7M4X5C0RNB8DA","display_name":"RTX PC","os":"LINUX",
            "daemon_version":"0.0.1","capabilities":["SESSIONS","EVENTS","METRICS","ADOPT"]},
 "endpoint":{"address":"100.74.239.89","port":7420,"dns_name":"pop-os.example.ts.net"},
 "control_credential":"<64 hex, generated by the worker>"}
→ 200 {"link_id":"lk_01J9Z4B2","control_machine_id":"01J9Y0CTRL0000000000000000",
       "worker_credential":"<64 hex>","credential_expires_at":1798676540000,
       "display_name":"RTX PC","heartbeat_interval_ms":30000}
→ 4xx {"code":"INVALID_CODE"|"PAIRING_LOCKED"|"INVALID_MACHINE"|"NOT_CONTROL","message":"…"}
   // a wrong, expired, used, or cancelled code all return INVALID_CODE (no oracle)

// POST /api/v1/link/heartbeat   Authorization: Bearer <worker_credential>
{"machine_id":"01J9Z3K8V6Q2T7M4X5C0RNB8DA","sequence":42,"sent_at":1790900590000,
 "endpoint":{"address":"100.74.239.89","port":7420,"dns_name":"pop-os.example.ts.net"},
 "daemon_version":"0.0.1","capabilities":["SESSIONS","EVENTS","METRICS","ADOPT"]}
→ 200 {"status":"ACTIVE","display_name":"RTX PC","next_heartbeat_ms":30000,"refresh_due":false}
→ 401 {"code":"LINK_REVOKED"}   → the worker deletes its link and reports "revoked"
→ 409 {"code":"STALE_SEQUENCE"}

// POST /api/v1/link/refresh     Bearer <worker_credential>
{"machine_id":"01J9…","control_credential":"<new 64 hex>"}
→ {"worker_credential":"<new 64 hex>","credential_expires_at":1806452540000}
// both directions rotate atomically; the old worker_credential works for 60 s more

// POST /api/v1/link/disconnect  Bearer <worker_credential>   {"machine_id":"01J9…"}  → 204
```

**Control → worker** (used by 4.A; 4.0 implements only identity)

```jsonc
// GET http://100.74.239.89:7420/api/v1/link/identity   Authorization: Bearer <control_credential>
{"machine_id":"01J9Z3K8V6Q2T7M4X5C0RNB8DA","control_machine_id":"01J9Y0CTRL0000000000000000"}
```

**Local socket (CLI ↔ local daemon)**, as new `LocalRequest` variants:
`GET_IDENTITY`, `CONNECT_CONTROL {control_url, pairing_code, display_name?}`,
`CONNECTION_STATUS`, and `DISCONNECT_CONTROL`.

**Existing `Machine` type.** It gains an optional `role`
(`CONTROL | WORKER`), serialized only when present. Existing fixtures
round-trip unchanged.

### 7. Direction, addresses, liveness, replay

- **Worker to control.** The worker always dials out, for exchange,
  heartbeat, refresh, and disconnect, to the control's `https://*.ts.net`
  origin. That is the same validated URL shape as `sentinal update`: no
  credentials, path, or port.
- **Control to worker.** The control reaches the worker (4.A) at the
  `endpoint` from the latest authenticated heartbeat. The address must be in
  `100.64.0.0/10` or `fd7a:115c:a1e0::/48`. The traffic is plain HTTP inside
  WireGuard, the same as today's control proxy, and the control credential is
  sent only to such an address. The control first checks
  `GET /link/identity`: the reply must name the expected `machine_id`.
- **Address changes.** A new address arrives in the next heartbeat. The
  control updates it and audits `ENDPOINT_CHANGED` with the old and new
  address.
- **Liveness.** Heartbeats every 30 s with ±10 % jitter. `last_seen` is the
  arrival time of the last accepted heartbeat. A Machine is `OFFLINE` after
  90 s without one (computed from the injected clock when read, never
  stored). After 3 failed heartbeats the worker reports
  `control unreachable` in `connection status`.
- **Replay.** TLS stops replay on the wire. Each heartbeat also carries a
  strictly increasing `sequence`, persisted on both sides. The control
  rejects `sequence <= last_sequence` (`409 STALE_SEQUENCE`) and any
  `sent_at` more than 5 minutes from its clock. Exchange is single-use by
  construction.

### 8. Registry and credential storage

Each side stores only a **hash** of the credential it verifies, and the
plaintext of the credential it presents:

| Credential | Generated by | Presented by | Plaintext stored | Hash stored |
|---|---|---|---|---|
| `worker_credential` (256-bit) | control | worker → control | worker `link.json` | control registry (SHA-256) |
| `control_credential` (256-bit) | worker | control → worker | control `machine-credentials.json` | worker `link.json` (SHA-256) |

Each credential is unique per link. They expire after 90 days and refresh
automatically when 30 days remain.

**Control, SQLite (store migration 5; data directory `0700`, database `0600`):**

```sql
CREATE TABLE registry_machines (
  machine_id TEXT PRIMARY KEY, display_name TEXT NOT NULL, role TEXT NOT NULL,
  os TEXT, daemon_version TEXT, capabilities_json TEXT NOT NULL DEFAULT '[]',
  endpoint_json TEXT, status TEXT NOT NULL,          -- ACTIVE | DISCONNECTED | REVOKED
  paired_at INTEGER NOT NULL, last_seen INTEGER, last_sequence INTEGER NOT NULL DEFAULT 0);
CREATE TABLE registry_links (
  link_id TEXT PRIMARY KEY, machine_id TEXT NOT NULL REFERENCES registry_machines,
  worker_credential_sha256 TEXT NOT NULL, created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL, previous_sha256 TEXT, previous_valid_until INTEGER,
  revoked_at INTEGER);
CREATE TABLE pairing_codes (
  code_id TEXT PRIMARY KEY, code_hmac TEXT NOT NULL UNIQUE, created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL, consumed_at INTEGER, cancelled_at INTEGER,
  machine_id TEXT);
CREATE TABLE pairing_failures (at INTEGER NOT NULL);   -- control-wide window
```

**Control, `~/.config/terminal-agent/machine-credentials.json`** (`0600`,
atomic rename). These are secrets the control must present, kept out of
SQLite so that a database shared for debugging contains none:

```json
{"version":1,"links":{"lk_01J9Z4B2":{"machine_id":"01J9Z3K8V6Q2T7M4X5C0RNB8DA","control_credential":"<64 hex>"}}}
```

**Worker, `~/.config/terminal-agent/link.json`** (`0600`, atomic rename):

```json
{"version":1,"control_url":"https://sentinal-control.example.ts.net",
 "control_machine_id":"01J9Y0CTRL0000000000000000","link_id":"lk_01J9Z4B2",
 "worker_credential":"<64 hex>","control_credential_sha256":"<64 hex>",
 "credential_expires_at":1798676540000,"display_name":"RTX PC",
 "paired_at":1790900540000,"sequence":42,"state":"ACTIVE"}
```

The browser receives only its owner Session for the control daemon and the
secret-free registry JSON above. Worker credentials and Machine tokens
never appear in any browser response, URL, or storage. Tests assert this
against every registry response.

**Revocation is isolated.** Revoking Machine A:

- marks A's link revoked;
- deletes A's `control_credential`;
- rejects A's next heartbeat with `LINK_REVOKED`.

It leaves Machine B, every browser Session, and A's local Sessions untouched.

### 9. Owner password interaction

- **First bootstrap.** Unchanged: the Machine token is exchanged for the
  first password on the control.
- **Already-configured control.** Add Machine needs only a valid owner
  Session.
- **Password change.** Revokes every browser Session (unchanged), cancels
  outstanding pairing codes, and leaves Machine links alone. Links
  authenticate Machines, not people.
- **Browser Session expiry.** The open dialog returns to login. Outstanding
  codes stay valid until their own expiry, and the owner can cancel them
  after logging in again.
- **Lost control browser.** Log in from another browser and cancel codes.
  Every outstanding code dies within 10 minutes anyway. Links are
  unaffected.
- **Workers keep their own password and Machine token** for direct use. In
  4.0, connecting never disables either.

### 10. Transport, origins, ACLs

- **Production.** The control daemon's API binds `127.0.0.1:7420` and is
  published by `tailscale serve --set-path /api http://127.0.0.1:7420`
  alongside the package role and the UI. The browser and workers use
  `https://<control>.<tailnet>.ts.net`.
- **Allowed origins.** A new `allowed_origins` setting in `daemon.toml`
  holds exact `scheme://host[:port]` strings.
  - Matching is exact and case-insensitive on scheme and host, with
    `https:443` and `http:80` normalized away. Suffix and prefix matches
    never count, so `https://control.example.ts.net.evil.com` and
    `https://evil-control.example.ts.net` fail.
  - The values `*`, `null`, and any entry with a path, query, userinfo, or
    wildcard character are refused when the config loads.
- **How requests are checked.**
  - A request with an `Origin` header must match the list, on every HTTP
    route and on the WebSocket upgrade. Otherwise it gets `403
    ORIGIN_REJECTED`.
  - `Origin: null` is always rejected.
  - Requests without `Origin` (CLI, worker links, curl) stay allowed, because
    they carry no ambient browser credential.
  - An **empty list** therefore blocks every browser and allows only
    non-browser clients. That makes it secure by default.
- **Development.** Plain HTTP origins such as `http://100.x.y.z:5173` are
  accepted only when `development = true`, which only `just network`, `just
  agent`, and `just control` write. Production config refuses `http://`
  origins.
- **Tailscale Serve identity.** When the TCP peer is loopback and
  `behind_tailscale_serve = true`, the login lockout keys on the
  `Tailscale-User-Login` header instead of `127.0.0.1`. This fixes audit
  finding #3 for this deployment. The header is ignored from any other peer.
- **ACL (documented in SETUP).**
  - Owner devices may reach `tag:sentinal-control:443`.
  - Workers (`tag:sentinal-worker`) may reach `tag:sentinal-control:443`.
  - The control may reach `tag:sentinal-worker:7420`.
  - Nothing else.

### 11. Audit and redaction

The control appends to `pairing-audit.jsonl` (`0600`). Each record holds
`{at, kind, code_id?, machine_id?, link_id?, peer?, reason?}`. The kinds are:

- `CODE_CREATED`, `CODE_CANCELLED`, `CODE_EXPIRED` (written lazily when the
  code is next touched or at startup);
- `EXCHANGE_SUCCEEDED`, and `EXCHANGE_FAILED` with a reason of
  `INVALID_CODE`, `LOCKED`, or `INVALID_MACHINE`;
- `LINK_REPLACED`, `HEARTBEAT_REJECTED`, `ENDPOINT_CHANGED`, `RENAMED`,
  `REVOKED`, `DISCONNECTED`, `CREDENTIAL_REFRESHED`, `ORIGIN_REJECTED`.

The worker appends the matching `link-audit.jsonl`.

Redaction rules:

- Never recorded anywhere: pairing codes, credentials, HMACs or hashes,
  Authorization headers, or request bodies.
- `peer` is the Tailscale user or IP only.
- Secrets travel as a `Secret` newtype with redacted `Debug` and `Display`.
  Error enums take no secret arguments.
- A test sweeps every audit, log, and error string produced by the
  failure-path tests for planted secret values.

### 12. Compatibility and migration

- **Protocol.** `PROTOCOL_VERSION` stays `1`. Only routes, `LocalRequest`
  variants, and one optional `Machine.role` field are added. No existing
  fixture changes, and new fixtures cover every new type.
- **Existing single-Machine login.** Unchanged. A browser logged into any
  daemon still sees that daemon's Sessions.
- **Control dashboard.** On a daemon with `[control]` enabled, Settings →
  Machines shows the registry and Add Machine. The control **registers
  itself** at startup as `role: CONTROL`: rename is allowed, revoke is not,
  and it has no credentials.
- **`run-control.sh WORKER_IP=` proxy mode.** Kept as a deprecated
  development path until 4.A replaces it with routing over links.
- **Data migrations.** Store migration 4 (Machine ID slug → ULID) and
  migration 5 (registry) run automatically. Neither touches Sessions beyond
  the `machine_id` rewrite.

### 13. CLI semantics and exit codes

| Command | 0 | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|---|
| `connect URL [--name N] [--code-stdin]` | connected | internal | usage, `--code` form, non-TTY | control unreachable / TLS / not a control | code rejected or pairing locked | already connected | local daemon unavailable or identity corrupt |
| `connection status [--json]` | connected, heartbeat OK | not connected | usage | connected but control unreachable | revoked | — | local daemon unavailable |
| `disconnect` | link removed (warns if the control could not be told) | not connected | usage | — | — | — | local daemon unavailable |

- **Already connected.** `connect` refuses (exit 5) and names the control URL
  and Machine ID, even for the same control: `sentinal disconnect` first. A
  revoked or disconnected worker reconnects with a new code and keeps its
  Machine ID and the owner's display name.
- **Retry after a wrong code.** The prompt allows 3 attempts in one run. Each
  attempt counts against the control's budget.
- **Code expires mid-exchange.** Exit 4, `that code expired; create a new one
  in Settings → Machines`.

## Dependency decisions needing approval

| Crate | Where | Why | Alternative considered |
|---|---|---|---|
| `sha2` + `hmac` (RustCrypto) | daemon | Credential hashes and code HMAC; small, audited, pure Rust | Argon2 for codes: slow and nonstandard for 256-bit secrets |
| `reqwest` (`rustls-tls`, `json`; no default features) | daemon | HTTPS client for exchange, heartbeat, refresh, and disconnect, with timeouts and typed errors | `curl -K -` subprocess with secrets on stdin: no new crate, but heartbeats spawn a process every 30 s and errors are stringly typed |

## Not in 4.0

- Session aggregation and routing (4.A).
- Scoping the control credential beyond `/link/identity` (4.A defines it).
- Signed releases.
- Mobile app.

## Implementation record (2026-10-02, macOS 27.0 arm64)

Everything above was implemented as approved, with these recorded
deviations:

| Approved text | As built | Why |
|---|---|---|
| `pairing_key` in `auth.json` | In `machine-credentials.json` (0600), beside the control credentials | `auth.json` exists only after password setup; keeping all control secrets in one owner-only file is simpler and still outside SQLite |
| Store migration 4 rewrites the slug, 5 adds the registry | Migration 4 adds the registry; the slug → ULID rewrite is an idempotent startup step driven by `legacy_machine_id` in the identity file | The rewrite needs the runtime ID, which SQL migrations do not have |
| Hidden prompt restores the terminal on `SIGINT` | The prompt runs with `-echo -icanon -isig`, reads byte by byte, and treats Ctrl-C/Ctrl-D as cancel before restoring | No signal handler is needed, so there is no window where echo stays off |
| `CODE_EXPIRED` written lazily | Written once per code (`expiry_audited_at`) when codes are next created, polled, or exchanged | Matches the design; recorded so the mechanism is clear |

Verification:

- Protocol: hand-written fixtures `machine_link.json`, `local_link.json`,
  `machine_control.json`; every code state, registry status, failure code,
  and local error is covered; old `machine.json` still round-trips without
  `role`; `Debug` of every secret-bearing type is redacted.
- Store: `registry.rs` tests for one-use consumption, expiry, cancel,
  failure windows, reconnect (owner name kept, old link revoked), isolated
  revocation, sequence replay, rotation grace, control self-registration,
  disconnect, expiry audit, and the Machine ID rewrite.
- Daemon: `identity` (7), `origin` (7), `config` (4 new), `control` (20,
  including a two-thread race on one code), `link` (16, real loopback HTTP
  between a control router and a worker), `enrollment_api` (7: owner-only
  routes, `NOT_CONTROL`, origins on HTTP and WebSocket upgrades, password
  change cancels codes, Tailscale-user lockout and spoofed-header refusal,
  worker identity route).
- CLI: `connect` (14: argv screening including the real binary, hidden-input
  echo off/on through an injected terminal, backspace, Ctrl-C, bounds, read
  errors, retries, `--code-stdin` bounds, every exit code, status JSON,
  disconnect warning, identity-reset confirmation).
- Web: registry client (4), Machines tab and Add Machine dialog (9), updated
  mock-mode Settings and real-mode availability tests; 102 Vitest tests.
- Playwright (5): a control and two worker daemons with distinct
  credentials; both workers enroll through the real `sentinal connect
  --code-stdin`, share a display name, are renamed and reloaded, and one is
  revoked while the other stays online; browser storage holds only the owner
  Session and theme.
- Launchers write `development = true` and exact dev origins (`just network`
  is a development control; `just agent CONTROL_IP=` allows `just control`).

Remaining coverage:

- `enrollment_tailnet_integration` (ignored) needs a control behind real
  Tailscale Serve and an owner-issued code; not run (no second host online,
  and it would change this Machine's Serve configuration).
- The Linux row was not run (no Linux host available).
- Production HTTPS UI hosting is not shipped; browsers still use the Vite
  development server (security audit item #6).
- Existing installs keep their `daemon.toml`; owners must add
  `allowed_origins` before a browser can reach an upgraded daemon.
