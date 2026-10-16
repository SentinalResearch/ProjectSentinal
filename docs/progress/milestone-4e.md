# Milestone 4.E progress — V1 installation, discovery, and private pilot

Status: **APPROVED as proposed by the owner on 2026-10-12**, including 11b
(control updates). All recommended answers to the questions below are
accepted:
- owner-held SSH release keys (primary plus offline backup), signed locally;
- archives plus `install.sh` for V1, deferring `.deb`, `.pkg`, Homebrew, and apt;
- GitHub Releases as the bootstrap URL;
- callback proof and `tailscale whois` both required before approval;
- live FULL Sessions block an update unless the owner explicitly confirms;
- control update policy Notify by default, with Automatic when idle opt-in
  and a daily api.github.com check;
- the `sentinal-pilot` VM, wiped, as the acceptance control.

Implementation in progress (see "Implementation order"). No signing key has
been created by the assistant; the owner generates and pins it.
Prompt:
`prompts/05e-milestone-4e-v1-install-discovery-private-pilot.md`.

Ordering: lands after Milestone 4.D.1, which is in progress in another
session and owns store migration `007_projection`. 4.E's migration is `008`,
and its heartbeat fields sit beside 4.D.1's additive `projection_sync`.

## What exists today (inspected)

| Area | Today | Gap for 4.E |
|---|---|---|
| Release build | `.github/workflows/release.yml` builds all 4 targets on GitHub runners, adds Sigstore build provenance and `SHA256SUMS`, and creates a draft Release | Provenance verification needs `gh` 2.49+ and a GitHub login. No signature that a shell script or a worker can check |
| Control receive | `scripts/receive-release.sh`: download, `SHA256SUMS`, `BUILD.json`, `gh attestation verify` (or explicit `--hash-only`), publish, self-update | Run from a git checkout; no independent signature (fixed in step 1) |
| Private packages | `sentinald` package listener on loopback `:7422` with `stable` channel, `install.sh`, immutable artifacts, rollback pointer | No `beta`; no retention bound; workers only learn of updates by running `sentinal update --check` |
| Worker install | `install.sh` from the control: target, length, SHA-256, safe archive entries, user service, PATH hint, real link state | Trust is the control's HTTPS origin plus a checksum from the same origin |
| Enrollment | 4.0 one-time code (50 bits, HMAC at rest, single use, rate limited). Exchange trusts the claimed `machine_id` because the code is owner authority | No discovery, no approval, no verification words |
| Setup | `sentinal doctor` (read-only; real role and link since 0.1.16) | No `sentinal setup`; a control is configured by hand-editing 17 lines of TOML (deployment-check finding 3) |
| Pilot | `docs/progress/deployment-check.md`: VM control on the Tailnet; manual steps; 12 findings | No runbook, preflight, backup, or rebuild procedure |
| Audit | F1, F5, F9, F10 open in `docs/AUDIT_TRACKER.md` | F10 matters here: the `Tailscale-User-Login` header is spoofable from loopback, so 4.E never keys security on it |

## 1. Release trust and initial bootstrap

**Trust root: an owner-held release signing key, checked with OpenSSH.**
Every release's `SHA256SUMS` carries an SSH signature
(`ssh-keygen -Y sign -n sentinal-release`). The owner makes it on their own
computer after reviewing the draft GitHub Release. The public key is pinned in:
- `install.sh`;
- every installed `sentinal` binary;
- the control's package service.

Verification is `ssh-keygen -Y verify` against a pinned `allowed_signers`.
OpenSSH 8.0+ ships with macOS and Ubuntu, and was proven here with a throwaway
key: a good signature verified, and one appended byte was refused.

| Property | Decision |
|---|---|
| Independence | The signature is made off GitHub and off the control. A compromised GitHub account, workflow, or control cannot produce a valid `SHA256SUMS` |
| Provenance | Kept as a second check: GitHub's Sigstore attestation proves *which workflow built it*. The owner's signature proves *the owner released it*. `receive-release` checks both by default |
| Key storage | An Ed25519 SSH key, preferably on a hardware key (`ssh-keygen -t ed25519-sk`), or a passphrase-protected file kept off servers. Never in GitHub Secrets, CI, the repo, or the VM |
| Rotation | `allowed_signers` lists a primary and an offline backup key from day one. A rotation release, signed by the current key, ships the new pinned list |
| Compromise | Sign an emergency release with the backup key. It removes the compromised key from `allowed_signers`, and every update verifies against the new pin. If both keys are lost, reinstall from a manually verified archive (documented) |
| Rollback | Rollback uses only releases that verified when published. A rollback is an explicit, signed, audited channel move |
| What is not claimed | Not Apple-notarized and not a Windows Authenticode signature. The first `curl … \| sh` trusts github.com HTTPS for the script itself, so the review-first form (download, verify, run) is the documented secure path |

**Bootstrap vs Tailnet packages**

```text
Owner laptop ── signs SHA256SUMS ──► GitHub Release v0.1.18 (public, signed + attested)
                                          │  (only the control and first installs read GitHub)
first control: curl install.sh ──────────►│  verify ssh sig (pinned key) + sha256 + length + target
                                          ▼
control: `sentinal release receive 0.1.18`  verify sig + attestation → publish to Tailnet channel
                                          ▼
workers: https://<control>.ts.net/install.sh, `sentinal update`   verify the same ssh sig + sha256
         (workers never contact GitHub; the package service grants no pairing authority)
```

No new dependency, external account, paid credential, or CI authority is
needed. Signing stays a manual owner step, by design.

## 2. Supported installers

| Deliverable | V1 decision | Why |
|---|---|---|
| Signed release archive `sentinal-<v>-<target>.tar.gz` | **Now** | One verified layout; it is the installer core |
| `install.sh` (`curl \| sh` and review-first form) | **Now**, the normal path | User-level `~/.local`, no root except Linux `enable-linger` (one prompt) |
| `.deb` (Ubuntu 22.04/24.04, Debian 12, Pop!_OS 22.04/24.04) | **Deferred** (question Q2) | A system package puts root-owned files in `/usr`. Sentinal is a per-user service with per-user data, so `.deb` would need a second install layout and a postinst that must not start user services. That doubles the installer safety surface |
| macOS `.pkg` | **Deferred**: needs an Apple Developer ID and notarization (paid) | An unsigned `.pkg` triggers Gatekeeper warnings, which is worse than the archive path |
| Homebrew tap, apt repository | **Deferred** | They need hosting and a second signing path |
| `pip` | **Never** | Native daemon, shell assets, and services |

Supported targets and honest refusals:

| Target | Support | Refusal text otherwise |
|---|---|---|
| macOS 13+ arm64 / x86_64 | yes | `Sentinal needs macOS 13 or newer (this is 12.7)` |
| Linux x86_64 / aarch64 with systemd user sessions (glibc or musl host; binaries are static musl) | yes | `Sentinal needs a systemd user session (systemctl --user is not available)` |
| WSL, FreeBSD, 32-bit, Alpine without systemd | no | `unsupported target 'FreeBSD:amd64'; supported: macOS 13+ (arm64, x86_64), Linux with systemd (x86_64, aarch64)` |

## 3. `sentinal setup` (guided, idempotent)

```text
$ sentinal setup
Sentinal 0.1.18 (abc1234) · Linux x86_64 · Tailscale: sentinal-pilot.tail….ts.net (100.88.176.80)

What is this Machine?
  1) Create my Sentinal control      (this Machine serves the dashboard and packages)
  2) Connect to my existing Sentinal (this Machine runs Sessions for a control)
  3) Advanced: enter a control URL or use a one-time code
Choose 1-3 (q to quit): 1

Name for this control [Advanced's Sentinal]:
Preview — nothing has changed yet:
  daemon        127.0.0.1:7420 (API)   127.0.0.1:7422 (PWA + packages)   never 0.0.0.0
  config        ~/.config/terminal-agent/daemon.toml   (created; existing file kept: none)
  data          ~/.local/share/terminal-agent/          backup boundary: config + data dirs
  service       ~/.config/systemd/user/sentinald.service  (restart: interrupts live FULL Sessions)
  keep running  sudo loginctl enable-linger sohith       (asks for your password once)
  Tailscale     https://sentinal-pilot.tail….ts.net/     → 127.0.0.1:7422
                https://sentinal-pilot.tail….ts.net/api  → 127.0.0.1:7420/api      Funnel: never
  dashboard     https://sentinal-pilot.tail….ts.net  (set the owner password there)
Apply? [y/N]: y
  ✓ config  ✓ service  ✓ linger  ✓ Serve  ✓ health {"ok":true}  ✓ release 0.1.18 published
Done. Open https://sentinal-pilot.tail….ts.net on a Tailnet device to set your password.
```

```text
$ sentinal setup            # on a worker
Looking for Sentinal controls on your Tailnet… (3 peers checked, 2 s)
  1) Advanced's Sentinal   https://sentinal-pilot.tail….ts.net   0.1.18 · control 01K2…
  2) Lab Sentinal          https://lab-mini.tail….ts.net         0.1.17 · control 01J9…
  3) Enter a URL / use a one-time code
Choose 1-3: 1
This Machine: "sohiths-macbook-pro" (01M40Q6X…). Name to show [Sohith's MacBook]:
Asked Advanced's Sentinal to approve this Machine.
Check that the dashboard shows the same words, then press Approve there:

        amber · otter · cedar · lumen

Waiting for approval… 9:41 left (Ctrl-C cancels; nothing is changed until approved)
  ✓ approved  ✓ credentials saved  ✓ heartbeat accepted  ✓ updates from sentinal-pilot (stable)
Connected as "Sohith's MacBook".
```

| Rule | Decision |
|---|---|
| Idempotent | Re-running shows the current role and offers only what is missing ("Already connected to Advanced's Sentinal. Nothing to do."). It never replaces an existing `daemon.toml`. A control/worker role conflict is refused with the exact file named |
| Config writes | Only missing keys, written to a temp file, previewed, then atomically renamed. The previous file is kept as `daemon.toml.before-setup` |
| Cancellation | Ctrl-C or `q` at any prompt changes nothing. During approval wait, Ctrl-C sends `DELETE` for the request (with the request secret), and the worker stores no credential |
| Privileges | No root, except Linux `loginctl enable-linger` (shown, confirmed, run with `sudo`) and Tailscale Serve where the OS requires `sudo tailscale serve` (shown first) |
| Password | Never asked by `setup`; the owner sets it in the dashboard (existing bootstrap). Codes use the existing hidden TTY input |
| Automation | `--role control\|worker`, `--control-url URL`, `--name NAME`, `--yes` (applies the preview), `--code-stdin` (fallback), `--json` (final status). `--yes` never approves pairing; approval is always human, on the control |
| Exit codes | 0 done · 1 internal · 2 usage/cancelled · 3 control unreachable · 4 rejected/expired · 5 already configured differently · 6 prerequisite missing (Tailscale, systemd user session) |
| Accessibility | Plain numbered prompts, no colour-only meaning, no spinners when not a TTY; every step prints a ✓ line or one recovery sentence |
| Failure | The changed files are rolled back to the `before-setup` copies. Failure prints one recovery step, e.g. `Tailscale Serve was refused (needs sudo). Run: sudo tailscale serve … then sentinal setup` |

`sentinal doctor` (extending 0.1.16) adds: control reachability (an
authenticated heartbeat age), package channel, available update, last update
result, and backup age on a control.

## 4. Tailscale discovery

1. Find the CLI: `tailscale` on PATH, then the macOS app bundle, or `SENTINAL_TAILSCALE_CLI`.
2. Run `tailscale status --json` (5 s timeout, 8 MiB cap). It must report `BackendState: Running`.
3. Candidates: `Peer` entries that are `Online`, with a `DNSName` ending
   `.ts.net.` and at least one 100.64/10 or fd7a:115c:a1e0::/48 address. No
   CIDR scan, no mDNS, no LAN addresses.
4. Probe `https://<DNSName>/api/v1/discovery`: at most 8 concurrent, at most
   64 peers, 2 s connect+read each, whole-run deadline 6 s. TLS is verified
   (Tailscale certs are public). No redirects. Body cap 4 KiB, JSON only.
5. Accept only when the response `origin` equals the probed origin and its
   host resolves to that peer's Tailscale address (no rebinding).
6. Edge cases:
   - **Duplicate display names:** show the host and a short control ID.
   - **Several controls:** list them all; never auto-pick.
   - **Older control (404):** list it as "older Sentinal — use URL + code".
   - **Offline peer:** skipped.
   - **ACL refusal (timeout or reset):** not listed; a summary line says
     "2 peers did not answer".
   - **Tailscale missing:** exit 6, with an install link.
   - **No controls:** fall back to option 3, URL + code.

## 5. Discovery resource

`GET /api/v1/discovery` is unauthenticated. It is served only when `[control]`
is enabled and the daemon is behind Serve on a `*.ts.net` public URL, and it
is reachable only through Tailscale Serve, because the daemon binds loopback.

```json
{"sentinal":"control","schema_version":1,
 "control_machine_id":"01K2CTRL00000000000000000",
 "display_name":"Advanced's Sentinal",
 "origin":"https://sentinal-pilot.tail17dafb.ts.net",
 "release":"0.1.18","protocol":{"min":1,"max":1},
 "pairing":{"approval":true,"code":true}}
```

No token, password state, owner, user list, Machine list, Session, path, or
subscription. Bounds:
- control-wide 60 requests per minute (not keyed on the spoofable
  `Tailscale-User-Login`, F10);
- response ≤ 1 KiB;
- `Cache-Control: no-store`.

A wildcard or public daemon cannot serve it: the control refuses a non-loopback
bind with `[control]` on, and refuses a non-`.ts.net` `public_url`.

## 6. Approval pairing contract (additive; `PROTOCOL_VERSION` stays 1)

**Identity proof.** The control does not trust the claimed Machine ID or hostname:
1. The worker sends its Tailscale endpoint.
2. The control calls back over WireGuard,
   `GET http://<endpoint>:7420/api/v1/pairing/proof/{request_id}`, the same
   control→worker path 4.A already uses. The worker answers only for a
   request that it created and that is still pending.
3. The control then runs `tailscale whois --json <endpoint>`. The Tailscale
   node name, owner login, and OS shown to the owner come from Tailscale's
   WireGuard identity, not from the request.
4. A request whose callback fails, or whose `whois` fails, is never
   approvable.

**Binding.** The worker creates a 256-bit `request_secret` and sends only its
SHA-256. Claiming the approved credential requires the secret, so a second
peer cannot claim an approval.

```jsonc
// worker → control   POST /api/v1/pairing/requests            (unauthenticated, via Serve)
{"machine":{"machine_id":"01M40Q6XMY22VM973434SCPS90","display_name":"Sohith's MacBook","os":"MACOS",
            "daemon_version":"0.1.18","capabilities":["SESSIONS","EVENTS","METRICS","ADOPT"]},
 "endpoint":{"address":"100.66.29.67","port":7420,"dns_name":"sohiths-macbook-pro.tail….ts.net"},
 "request_secret_sha256":"<64 hex>","worker_nonce":"<32 hex>"}
→ 201 {"request_id":"pr_01K2A7…","control_machine_id":"01K2CTRL…","control_nonce":"<32 hex>",
       "expires_at":1791900600000,"poll_after_ms":2000}
→ 409 {"code":"MACHINE_ALREADY_LINKED"}  → 429 {"code":"PAIRING_BUSY"}  → 404 {"code":"NOT_CONTROL"}

// control → worker   GET http://100.66.29.67:7420/api/v1/pairing/proof/pr_01K2A7…
{"request_id":"pr_01K2A7…","request_secret_sha256":"<same 64 hex>","control_machine_id":"01K2CTRL…"}

// owner → control    GET /api/v1/pairing/requests            (owner Session, Origin checked)
{"requests":[{"request_id":"pr_01K2A7…","state":"PENDING","expires_at":1791900600000,
  "claimed":{"display_name":"Sohith's MacBook","machine_id":"01M40Q6X…","os":"MACOS","daemon_version":"0.1.18"},
  "tailscale":{"node":"sohiths-macbook-pro","login":"sohithvishnu@","os":"macOS","address":"100.66.29.67"},
  "existing_machine":null,
  "verification_words":["amber","otter","cedar","lumen"]}]}
// existing_machine is present when the claimed ID is in the registry as REVOKED/DISCONNECTED:
// {"display_name":"MacBook","status":"REVOKED","last_seen":1791800000000} → shown as "reconnects MacBook"

// owner → control    POST /api/v1/pairing/requests/pr_01K2A7…/approve  {"display_name":"Sohith's MacBook"} → 200 {"state":"APPROVED"}
//                    POST …/reject → 200 {"state":"REJECTED"}

// worker → control   POST /api/v1/pairing/requests/pr_01K2A7…/claim
{"request_secret":"<64 hex>","control_credential":"<64 hex, new>"}
→ 202 {"state":"PENDING","poll_after_ms":2000}
→ 200 {"state":"APPROVED","link_id":"lk_…","control_machine_id":"01K2CTRL…","worker_credential":"<64 hex>",
       "credential_expires_at":1799676540000,"display_name":"Sohith's MacBook","heartbeat_interval_ms":30000}
→ 403 {"state":"REJECTED"}  → 410 {"state":"EXPIRED"}  → 401 {"code":"INVALID_CLAIM"} (wrong secret; counted)

// worker → control   DELETE /api/v1/pairing/requests/pr_01K2A7…  {"request_secret":"<64 hex>"} → 204 (cancel)
```

| Rule | Decision |
|---|---|
| Lifetime | 10 minutes from creation; at `expires_at` it is expired. Survives a control restart (SQLite) |
| One success | `UPDATE … SET claimed_at=? WHERE request_id=? AND state='APPROVED' AND claimed_at IS NULL AND expires_at>? AND secret_sha256=?`, in the same transaction as the registry write and link creation (the existing exchange internals) |
| Claimed ID | If the claimed `machine_id` has an ACTIVE link, the request is refused (`MACHINE_ALREADY_LINKED`). A REVOKED or DISCONNECTED ID is shown as "reconnects <name>" with its last-seen time, never silently merged |
| Rate limits (control-wide; F10) | At most 5 PENDING; 20 new requests per 10 min; 20 failed claims per 10 min, then `429 PAIRING_LOCKED`, which cancels all pending requests |
| Fallback | The 4.0 code flow and its routes are unchanged (`sentinal setup` option 3, `sentinal connect`) |
| Audit (`pairing-audit.jsonl`, metadata only) | `REQUESTED`, `PROOF_OK/FAILED`, `APPROVED`, `REJECTED`, `CANCELLED`, `EXPIRED`, `CLAIMED`, `CLAIM_REFUSED{reason}`, `ALREADY_LINKED`. Records hold request ID, Machine ID, Tailscale node, and reason. Never secrets, nonces, request bodies, or Tailnet auth headers |

**Storage — store migration `008_pairing_requests`:**

```sql
CREATE TABLE pairing_requests (
  request_id TEXT PRIMARY KEY, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
  state TEXT NOT NULL,                       -- PENDING | APPROVED | REJECTED | CANCELLED | EXPIRED | CLAIMED
  secret_sha256 TEXT NOT NULL, worker_nonce TEXT NOT NULL, control_nonce TEXT NOT NULL,
  claimed_machine_json TEXT NOT NULL, endpoint_json TEXT NOT NULL,
  tailscale_json TEXT, proof_ok INTEGER NOT NULL DEFAULT 0,
  approved_name TEXT, decided_at INTEGER, claimed_at INTEGER, machine_id TEXT);
CREATE TABLE pairing_claim_failures (at INTEGER NOT NULL);
```

Generated TypeScript (additive):
- `DiscoveryResponse`;
- `PairingRequestCreate`/`Created`, `PairingRequestView`, `PairingDecision`;
- `PairingClaim`/`Result`, `PairingProof`;
- fixtures for each.

## 7. Verification words

- **Derivation.** `SHA-256("sentinal-pair-v1" ‖ control_machine_id ‖ request_id
  ‖ control_nonce ‖ request_secret_sha256 ‖ worker_nonce)`, first 32 bits, read
  as 4 words from a fixed 256-word list. The list is embedded in `protocol`:
  short, distinct, easy-to-say English words; no dependency, data only.
  Worker and control compute the words independently.
- **Grinding.** The words cannot be ground ahead of time: the control's nonce
  arrives only after a request is created, and requests are limited to 20 per
  10 minutes. A 32-bit match by chance is about 2⁻³².
- **Concurrent requests.** Every pending request shows its own words. The owner
  approves the row whose words match the worker's screen. The UI never offers
  "approve all".
- **Wrong control.** The worker prints the chosen control's name and short ID
  next to the words. A different control produces different words.
- **No display.**
  - With `--json`, the worker prints the words in its JSON status line.
  - With no TTY and no `--json`, approval pairing is refused: use the code
    flow or `--json`.

## 8. Control identity and URLs

- **Identity:** the control's existing `machine_id`. Workers already store
  `control_machine_id`, and every discovery, heartbeat, and update response
  must carry the same ID.
- **Origin:** `https://<MagicDNS name>` on 443. If Serve already maps `/` on
  443 to something else, setup offers port 8443
  (`https://host:8443`). It never overwrites a foreign route, never runs
  Funnel, and never binds a LAN or public address.
- **Origin or hostname change:** `sentinal setup --reconnect` on a worker
  rediscovers controls, picks the one whose `control_machine_id` matches the
  stored one, and moves `control_url` and the update origin together. This
  fixes deployment-check finding 7, without re-pairing (the existing
  credential is accepted).
- **Restore:** a restore keeps `machine_id` and credentials, so workers
  reconnect unchanged. A rebuilt control without a backup has a new ID;
  workers refuse it and say "this is a different control; pair again".

## 9. Private cloud pilot

```text
 Internet ──X── (provider firewall: SSH 22 from owner IP only, or none + Tailscale SSH)
                  │
 Tailnet ─────► VM "sentinal-pilot" (Ubuntu 24.04, 2 vCPU, 4 GB, 40 GB)
                  tailscaled · Serve 443: / → 127.0.0.1:7422, /api → 127.0.0.1:7420/api
                  sentinald (user service, linger) · SQLite · packages · VAPID key
                  outbound: Web Push endpoints, github.com (releases), OS updates
 Workers ─────► 443 on the VM only;  VM ──► workers :7420 (control → worker routing)
```

- **Runbook:** `docs/PILOT.md`. Install, setup, verify, back up, restore, and
  rebuild, using only repository scripts. No Terraform, Docker, or cloud SDK.
- **Preflight:** `sentinal doctor --pilot` on the VM, read-only. It checks:
  - that only loopback listens on 7420/7422 (`ss`/`lsof`);
  - that Serve has exactly the two routes and Funnel is off;
  - HTTPS health, the PWA, and `install.sh`;
  - the channel version, the VAPID key mode, and backup age;
  - disk use and log sizes (F9 visibility).
- **Backup:** `sentinal backup create` stops the service, writes a 0600 tar of
  the config and data directories outside both, then restarts it. It warns
  that the archive holds credentials. `sentinal backup restore` works only
  onto a stopped install with no data.
- **Logs:**
  - journald for the service;
  - Sentinal's own logs stay unbounded, as recorded under F9. The pilot reports
    their sizes but does not hide the gap.
- **ACL example:** the grants already in `docs/SETUP.md`, plus `tagOwners` and
  a `tests` block that asserts each denial.
- **Not part of 4.E:** a public landing or download site (separate approval).

## 10. Private package manager

- **Channels:** `stable` and `beta` pointers. Releases are immutable and
  retained up to 5 per channel plus anything a channel points at.
  `release receive --channel beta` publishes to beta only.
- **Discovering updates:** the heartbeat response gains an additive
  `available_release:{version, channel}`, so no extra polling is needed.
- **Worker selection:** exact OS and architecture from the manifest; refuses a
  missing target.
- **Verification:** SSH signature over `SHA256SUMS`, then sha256, length,
  target, `BUILD.json` version, and minimum protocol.
- **Download:**
  - 256 MiB per artifact and 8 concurrent artifact streams (existing bounds);
  - a partial download is discarded and retried, never resumed (no `Range`);
  - when the control is unreachable, update reports it and changes nothing.

## 11. Update coordination

The owner presses Update on one Machine in the PWA. The confirmation sheet
names the Machine, the current → new version, and "N live FULL Sessions will
be interrupted (until Reliability R1)".

1. `POST /api/v1/machines/{id}/update {"version":"0.1.18","allow_interrupt":false}`
   (owner Session).
2. The control sends `POST http://<worker>:7420/api/v1/link/update
   {"version":"0.1.18","channel":"stable","allow_interrupt":false}` over the
   existing link, with the control credential. It carries only a validated
   version and a channel enum: no command, path, or URL.
3. The worker refuses with live FULL Sessions unless `allow_interrupt` is
   true (`409 LIVE_SESSIONS{count}`). Otherwise it fetches the manifest
   from its **own pinned control origin** and verifies everything
   independently. Then it stages the release, swaps it, restarts, and checks
   health (local socket and heartbeat within 60 s). If the check fails, it
   rolls back to the previous bundle automatically.
4. The worker reports `update_status`
   (`STAGED|INSTALLED|ROLLED_BACK{reason}|FAILED{reason}`) in later heartbeats.
   After 5 minutes without a report, the control shows `UNKNOWN`.
5. Mixed versions are fine within protocol 1. One Machine's failure never
   touches another. Every update and rollback is audited with Machine,
   versions, and result.

## 11b. Control updates (as easy as a worker's)

Today the control updates only when someone runs `receive-release.sh` in an
SSH session, and the dashboard shows nothing about it. 4.E makes the control
update itself the same way workers do, while the owner's release signature
still gates every release.

**1. Release watch.** The control checks
`https://api.github.com/repos/<repo>/releases` about every 24 h (± 1 h
jitter), after start, and when the owner presses "Check now".
- Unauthenticated, HTTPS only, at most 3 redirects, at most 256 KiB.
- It considers only releases on the subscribed channel: `stable` (published)
  or `beta` (pre-release).
- No candidate is accepted unless **its `SHA256SUMS.sig` verifies against the
  pinned release keys** (section 1). A draft, an unsigned release, or a
  wrongly signed one is never offered; it shows as "0.1.19 found but not
  signed by a trusted key".
- Offline or rate-limited: it keeps the last result and shows "last checked
  3 h ago".

**2. One-button update.** Settings → Machines shows the control's card as
"Control · 0.1.17 → 0.1.18 available", with release notes and an
**Update control** button. The confirmation sheet states:
- "The dashboard will reconnect for about 30 s.";
- "N live FULL Sessions on this control will be interrupted (until
  Reliability R1).";
- "Workers keep running and reconnect on their own."

**3. What the update does.** Each step is audited.

1. Download all target artifacts for the release; verify the signature,
   sha256, length, `BUILD.json`, and, when `gh` 2.49+ is present, GitHub
   provenance.
2. Publish them to the private channel (immutable; `stable` moves last). Once
   step 4 passes, workers see the release in their next heartbeat.
3. Stage this Machine's artifact. A small detached updater, `sentinal
   release apply` started through the user service manager, swaps the
   bundle and restarts `sentinald`. The daemon never replaces itself
   in place.
4. Health check: the new daemon must answer local and HTTPS health, open its
   database, and keep its `control_machine_id` within 60 s.
5. On failure, restore the previous bundle and restart. The channel pointer
   returns to the old release too, so no worker is offered a release the
   control rejected. The result is recorded as `ROLLED_BACK{reason}`.
6. The reconnected dashboard shows "Control updated to 0.1.18" or "Rolled
   back to 0.1.17: <reason>".

**4. Order and workers.** The control always updates first. A worker update
can never target a version newer than the control's (enforced by the
control). After the control succeeds, the sheet offers **Update workers…**:
one confirmation listing each online worker, its version, and its live-Session
count. It then runs the worker flow from section 11 one Machine at a time; a
failure stops the run and leaves the rest untouched.

**5. Update policy** (Settings → Updates; stored in the control's config):

| Policy | Behaviour |
|---|---|
| **Notify** (default) | Watch, notify the owner (Web Push "Sentinal 0.1.18 is available"), and wait for the button |
| **Automatic when idle** | In the owner's maintenance window (default 03:00–05:00 local), update the control only when it has no live FULL Sessions. Then update idle workers (no live FULL Sessions) one at a time. Busy Machines are retried the next night. Every result is pushed to the owner |
| **Off** | No GitHub requests; manual `sentinal release receive` only |

Automatic mode never installs an unsigned release, never interrupts a live
FULL Session, never downgrades, and never changes channel. A rollback in the
window pauses automatic updates until the owner acknowledges it.

**6. Terminal equivalents** (same code path):
- `sentinal release check` (prints current, available, and signature status);
- `sentinal release update [--version V] [--yes]`;
- `sentinal release rollback V`;
- `sentinal release policy notify|auto|off`.

These replace `scripts/receive-release.sh` for normal use, so the control no
longer needs a git checkout (deployment-check finding). The script remains for
recovery.

```jsonc
// GET /api/v1/control/release   (owner Session)
{"current":"0.1.17","channel":"stable","policy":"NOTIFY",
 "available":{"version":"0.1.18","signed":true,"provenance":"VERIFIED","published_at":1792000000000,
              "notes_url":"https://github.com/<repo>/releases/tag/v0.1.18"},
 "last_checked_at":1792003600000,"last_result":null,
 "live_full_sessions":2}
// POST /api/v1/control/release/update {"version":"0.1.18","allow_interrupt":true} → 202 {"update_id":"cu_01K…"}
// GET  /api/v1/control/release/updates/cu_01K… → {"state":"HEALTH_CHECK"|"UPDATED"|"ROLLED_BACK","reason":null}
```

## 12. Data lifecycle

These are kept across reinstall, upgrade, and default uninstall:
- Machine ID and link credentials;
- the password hash, VAPID key, and Push subscriptions;
- SQLite, Session logs, audits, recipes, adapters, and config.

Uninstall removes programs and the service only. `--delete-data PATH` deletes
only the exact resolved data directory, after the path is displayed (existing
behaviour). Backup covers the config and data directories. Restore needs a
stopped install with no data.

## 13. What this pilot is not

It is a private, single-owner, single-tenant pilot. It does not have:
- SSO, RBAC, organizations, or multi-user accounts;
- tenant isolation;
- compliance certification (SOC 2, ISO 27001);
- high availability, failover, or replicated control;
- billing, public ingress, a vendor relay, or support SLAs.

Release blockers F1, F5, F9, and F10 stay open unless fixed separately. It
must not be called production enterprise software.

## 14. Compatibility and dependencies

| Item | Effect |
|---|---|
| Protocol | Additive only. New discovery/pairing/update types; heartbeat response gains optional `available_release`; worker heartbeat gains optional `update_status`. `PROTOCOL_VERSION = 1`. New fixtures; existing fixtures unchanged |
| Store | Migration `008_pairing_requests` (after 4.D.1's `007`) |
| Generated TS | New files only; `just gen-ts --check` |
| Legacy worker (≤ 0.1.17) | Cannot do approval pairing; code flow still works; updates via `install.sh` as today |
| Legacy control | Discovery 404 → listed as "older Sentinal — URL + code" |
| Downgrade | An older binary ignores the new tables and fields; pending approval requests are lost (codes still work) |
| New dependencies | **None** in Rust or web. System tools: `ssh-keygen` (OpenSSH ≥ 8.0, preinstalled), `tailscale` (existing), `gh` ≥ 2.49 on the control for provenance only |
| Control release watch | Outbound HTTPS to `api.github.com` and `github.com` only, from the control; disabled by policy **Off** |
| Cannot run in CI | Owner signing (by design), Apple notarization (deferred), real Tailscale/VM/Serve acceptance |

## Failure messages (representative)

- `No Sentinal control found on your Tailnet (5 peers checked; 2 did not answer). Enter a URL or use a one-time code.`
- `sentinal-pilot answered as "Advanced's Sentinal" but its control ID changed (01K2… → 01K9…). This is a different control; pair again or restore it.`
- `This Machine is already connected to Advanced's Sentinal. Run 'sentinal disconnect' first to pair elsewhere.`
- `The request expired before it was approved (10 minutes). Run 'sentinal setup' to ask again.`
- `Release 0.1.18 is not signed by a trusted Sentinal release key; nothing was installed.`
- `Update refused: 2 live FULL Sessions would be interrupted. Confirm in the dashboard to proceed.`

## Recovery sequences

- **Failed update:** the worker rolls back automatically and reports
  `ROLLED_BACK`. The owner may pin the channel back with
  `sentinal release rollback 0.1.17`.
- **Lost control VM:** provision a VM, install, then `sentinal backup restore`,
  then `sentinal setup` (control). Workers reconnect on their own: same ID and
  credentials, as long as the MagicDNS name is the same. (`--reconnect` for a
  changed name is not built; such workers pair again.)
- **No backup:** a new control. It refuses the workers' heartbeats; re-pair
  each worker; Push devices subscribe again.

## Implementation order (after approval)

Each step is test-first and committed separately.

1. **Release signing.**
   - A pinned `allowed_signers` file and shared signature verification.
   - An owner signing tool that signs a release outside CI.
   - Signature checks in `receive-release`, the publisher, the package service
     (serving `SHA256SUMS` and `.sig`), and the control's `install.sh`.
   - A GitHub bootstrap `install.sh`.
2. **`sentinal release`** (check/update/rollback/policy), replacing
   `receive-release.sh`: the release watch, staged self-update, health check,
   and rollback (11b).
3. **`sentinal setup`** for CONTROL (preview/apply) and the setup half of
   `doctor --pilot`.
4. **Discovery:** the resource and the Tailnet client.
5. **Approval pairing:**
   - protocol types and fixtures;
   - migration 008;
   - routes, the callback proof, and `whois`;
   - verification words;
   - the WORKER side of `sentinal setup`.
6. **Pairing and update UI** in the PWA (desktop and phone).
7. **Worker update coordination** (section 11) and the beta channel and retention.
8. **Pilot runbook** (`docs/PILOT.md`), `sentinal backup`, and real-Machine
   acceptance.

## Implementation record

### Step 1 — release signing (2026-10-12)

Pin and verification:
- `packaging/release-keys/allowed_signers` is the pin. It ships with **no
  key**, so everything fails closed until the owner adds theirs.
- `scripts/lib/release-signature.sh` is the shared shell verification. It uses
  `ssh-keygen -Y verify` and parses `SHA256SUMS` strictly.
- `scripts/sign-release.sh` is the owner's local tool. It checks every listed
  file, signs, refuses a key that is not pinned, and prints the upload
  command. It never runs in CI.

Release tooling:
- `scripts/publish-worker-release.sh` requires `--sums` and `--signature`. It
  checks every artifact against the signed list and stores
  `releases/<v>.SHA256SUMS(.sig)`.
- `scripts/receive-release.sh` always downloads and verifies
  `SHA256SUMS.sig`. `--skip-provenance` skips only GitHub provenance;
  `--hash-only` was removed with an explanatory error.

Daemon:
- `PINNED_RELEASE_KEYS` is compiled from the pin. An optional owner-only
  `[packages] extra_release_keys` file adds keys (≤ 8 KiB, key lines only,
  not group- or world-writable).
- `GET /packages/v1/releases/<v>.SHA256SUMS[.sig]` serves the signed files
  (bounded, owner-only, no symlinks).
- The generated `install.sh` verifies the signature before downloading the
  archive and requires the manifest digest to equal the signed one.

GitHub bootstrap:
- `packaging/github-install.sh.in`, rendered by
  `scripts/render-github-install.sh` (refuses with no pinned key).
- It verifies the owner signature and installs the host's archive under
  `~/.local`.
- It refuses unsupported platforms before any download.

Release workflow:
- `.github/workflows/release.yml` renders `install.sh`, lists it in
  `SHA256SUMS`, and leaves an unsigned draft with signing instructions.
- The workflow test forbids `ssh-keygen`, `-Y sign`, and
  `gh release upload` in CI.

Tests:
- `release-signature-test.sh`;
- `github-install-test.sh` (mutation-checked: disabling the signature check
  fails it);
- signed cases in `package-release-test.sh` and `receive-release-test.sh`;
- `crates/daemon/tests/package_signatures.rs` (serving, extra keys, and
  install.sh refusals: no key, unpinned key, missing signature, tampered
  list, digest mismatch);
- unit tests in `release_keys.rs` and `signed.rs`.

Evidence: `just check` passed.

**Owner action before the next tag:** generate the primary and backup keys and
commit their public lines to `packaging/release-keys/allowed_signers`
(`docs/SETUP.md` → Release signing). Until then:
- the release workflow's render step fails by design;
- controls on ≤ 0.1.17 keep serving their old, unsigned install path.

### Step 2a — `sentinal release` on the control (2026-10-12)

Release tools:
- Archives ship them under `tools/`, laid out like the repository:
  `receive-release.sh`, `publish-worker-release.sh`, `apply-release.sh`,
  their libraries, and `release-keys/allowed_signers`.
- The installer installs them to `share/sentinal/tools`, with the same
  transactional restore as the other assets.
- New `scripts/apply-release.sh` reinstalls a published release from the local
  package directory. It re-verifies the stored signature, the artifact digest,
  and the archive entries. `receive-release` self-update and rollback both
  use it.

`sentinal release` (`crates/cli/src/release/`):
- **check:** reads the GitHub releases API through `curl`, then filters by
  channel (stable or beta), skips drafts, and keeps only versions newer than
  the installed one. It offers the newest release whose `SHA256SUMS`
  verifies against the pinned keys and lists all 4 targets. It reports, but
  never offers, an unsigned one.
- **update:** confirms, runs the receive tool, then polls `/api/v1/health`.
  If health fails, it moves the channel back and reinstalls the previous
  signed release, and records `ROLLED_BACK`.
- **rollback:** explicit; the release must be published locally.
- **policy:** notify, auto, or off, plus the channel.
- State is kept 0600 in `release-watch.json` and `release-policy.json`.
- A worker is refused.

Tests: `crates/cli/tests/release.rs` (8 cases; mutation-checked: removing the
signature check fails the unsigned case), apply cases in
`receive-release-test.sh`, tools in package and installer tests.

Next, 2b: the daemon's daily watch, the dashboard Update control button and
`GET/POST /api/v1/control/release`, Push on results, and automatic when idle.

### Step 2b — control-release contract (protocol layer only, 2026-10-12)

The wire contract for `/api/v1/control/release`, ahead of the daemon route
handlers, the daily GitHub watch, the dashboard button, and automatic-when-idle
(all still open — see "Next" above, unchanged):

- `crates/protocol/src/release.rs`: `ReleaseChannel` (stable/beta),
  `ReleasePolicy` (notify/automatic-when-idle/off), `AvailableRelease`,
  `ControlUpdateState` (running/updated/rolled-back/failed/unknown),
  `ControlUpdateStatus`, `ControlReleaseView` (the full dashboard payload —
  current version, channel, policy, window, available release, any unsigned
  release found, last-checked time, in-progress update, live-FULL-Session
  count), `ControlUpdateRequest` (version + `allow_interrupt`, strict
  `MAJOR.MINOR.PATCH` only), `ControlUpdateStarted`, `ReleasePolicyRequest`.
- `ProtocolError::InvalidReleaseVersion` for a malformed version string.
- Fixtures: `crates/protocol/tests/fixtures/release.json` (an available-update
  view and a rolled-back-update view). Round-trip test plus a dedicated test
  asserting `ControlUpdateRequest::validate()` rejects non-strict versions
  (missing/extra segments, a `v` prefix, a leading zero, injected shell text).
- Generated TS (`web/src/protocol/*.ts`) verified against this with
  `just gen-ts --check`.

Evidence: `cargo test -p protocol`, `cargo fmt -p protocol -- --check`,
`cargo clippy -p protocol --all-targets --all-features -- -D warnings`, and
`just gen-ts --check` all pass.

### Step 2b — daemon release watch (2026-10-12)

The control's background check for a newer signed release, the "about every
24h (± 1h jitter), after start" half of §11b point 1. Still open after this:
the `GET/POST /api/v1/control/release` routes, the dashboard Update-control
button, Push on results, and automatic-when-idle actually installing
anything — this piece only watches and logs.

Deliberately **not** a new shared crate and **no changes under `crates/cli`**:
`sentinal release check --json` (step 2a, already shipped, mutation-tested)
already does the entire fetch/channel-filter/signature-verify/record job.
Reimplementing or extracting that logic risked behavioral drift on a
signature-verification path for no benefit, so the daemon's job is only to
decide *when* to run it:

- `crates/daemon/src/control/release_watch.rs`: `spawn_release_watch`
  (`tokio::spawn`, runs one check promptly on start, then loops on
  `tokio::time::sleep` with a freshly jittered duration each cycle, racing a
  `tokio::sync::watch::Receiver<bool>` shutdown signal — same shutdown idiom
  as `spawn_notification_workers`). `run_one_check` reads just the `"policy"`
  field of `release-watch.json`'s sibling `release-policy.json` (missing/
  unparsable ⇒ `NOTIFY`, matching the CLI's own default) and, unless it is
  `OFF`, runs `sentinal release check --json` as a child process (120s
  timeout, on top of the CLI's own internal 30s per-request bound), logging
  the parsed result or any failure via `tracing` — never panics, never stops
  future scheduled checks.
- Jitter: `BASE_INTERVAL_SECS (24h) - 1h + (seed mod 2h+1s)`, `seed` from
  wall-clock sub-second nanoseconds. No `rand` dependency: this only needs to
  spread restart timing across machines, not be unpredictable.
- `crates/daemon/src/entry.rs`: spawns the watch only when `config.control`
  is `Some`, resolving the sibling `sentinal` binary via
  `current_exe()?.parent()`. If that resolution fails, logs
  `tracing::error!` and continues starting the daemon normally — a control
  still works without the watch, it just won't auto-discover updates.
- Tests (`crates/daemon/src/control/release_watch.rs`, inline, `#[cfg(unix)]`,
  reusing the fake-executable-on-disk technique already used in
  `crates/cli/tests/release.rs`): `OFF` policy never invokes anything;
  `NOTIFY`, `AUTOMATIC_WHEN_IDLE`, and a missing policy file all do invoke
  `release check --json` with exactly those arguments; a nonexistent binary,
  a nonzero exit, and unparsable stdout each log a warning and return without
  panicking (and a later valid check still succeeds afterward); the jitter
  function stays within ±1h across a spread of seeds including `0` and
  `u32::MAX`.

Evidence: `cargo fmt --all -- --check`, `cargo clippy --workspace
--all-targets --all-features -- -D warnings`, `cargo test -p daemon` (79 lib
tests + 8 new release-watch tests, all passing — one unrelated,
pre-existing, order-sensitive flake in `session_manager.rs`'s
`unterminated_training_output_persists_watcher_events_and_session_health`
reproduced and passed in isolation, confirming it predates and is unrelated
to this change), `cargo test -p cli` (unchanged, still passing — confirms
zero behavioral drift to step 2a), `cargo test --workspace`,
`scripts/tests/release-workflow-test.sh`, and
`scripts/tests/receive-release-test.sh` all pass.

### Steps 2b–2d — dashboard control updates (2026-10-12)

**Protocol (`bf67efe`, contract `crates/protocol/tests/release.rs`)**
- `ControlReleaseView`, `ControlUpdateRequest` (strict version),
  `ControlUpdateStarted`, `ReleasePolicyRequest`, plus the policy, channel,
  and update-state enums.
- `NotificationCategory::Release` (additive).

**Daemon**
- `c5dc88b` added the daily watch.
- `3f4bfc1` added `ControlRelease` and the owner-only routes:
  - `GET /api/v1/control/release`: files only;
  - `POST …/check` and `PUT …/policy`: run the CLI;
  - `POST …/update`: only the offered signed release, with consent for live
    FULL Sessions and none while one runs. It starts
    `sentinal release update` detached (`systemd-run --user` on Linux, a new
    process group on macOS) and tracks progress through
    `release-update.json` (RUNNING → result, UNKNOWN after 15 min).
- **Step 2d:**
  - `PushGateway::send_control_notice` sends best-effort Web Push to every
    active device. The only non-Session route is `/app/settings/machines`,
    enforced in `SafePushPayload` and in the service worker's
    `safeNotificationRoute`.
  - `release_actions::plan` (pure) announces each new signed release and each
    update result once (`release-notified.json`, 0600).
  - Under Automatic when idle, it starts the update only inside the window,
    with no live FULL Session and no update running. A version that already
    rolled back, failed, or ended UNKNOWN is never retried automatically.
  - The watch wakes every 15 min (the GitHub check stays daily) and reads
    local time via `date +%H%M`, so no time-zone dependency is added.

**Web (`58d0aef`)** — `ControlReleaseCard` in Settings → Machines, desktop and
phone:
- current version and channel; the offered signed release with notes; an
  unsigned warning; Check now;
- the policy and channel segmented controls;
- a confirmed update (ConfirmDialog on desktop, a sheet on phones) stating
  that the dashboard reconnects and how many Sessions would be interrupted;
- polls through the restart to "Updated" or a rollback alert.

**Tests:**
- `crates/daemon/tests/control_release.rs` (owner-only, check, policy,
  consent, offered-only, detached update to UPDATED, worker 404);
- `control_notice.rs`;
- 6 `release_actions` unit tests;
- `ControlReleaseCard.test.tsx` (6);
- `push-safety.test.ts` (the route).

**Not automated yet:** workers updating automatically after the control (step 7).

### Step 3 — `sentinal setup` (2026-10-12)

`crates/cli/src/setup/`:
- `config.rs` edits `daemon.toml` as text, so comments and other keys
  survive. `bind` moves to `127.0.0.1:7420`; `behind_tailscale_serve` and
  `allowed_origins` go above the first table; `[control]` (with
  `display_name`, which discovery will serve in step 4) and `[packages]` are
  appended. A config that already sets any of these by hand, binds somewhere
  other than loopback or Tailscale, or is a development config is refused
  (exit 5), never merged.
- `control.rs` plans and previews linger, the config, the restart, and the
  missing Serve routes, then applies them in that order.
  - A failed restart or health check restores the previous file and restarts
    again.
  - A Serve route that points elsewhere, raw TCP on 443, or Funnel on the
    host is refused before anything changes.
  - Last, it publishes the installed signed release (`receive-release.sh
    --no-self-update`; provenance is checked when `gh` can).
  - A finished control prints "Nothing to do".
- `system.rs` puts Tailscale, `loginctl`, `systemctl`/`launchctl`, `curl`, and
  publishing behind the `System` trait.
- The WORKER role validates the control URL and continues with the existing
  code flow (`connect`), until discovery and approval land in steps 4–5.

**Deviations from §3:**
- **No port-8443 fallback.** `public_url` and `public_base_url` are validated
  as port-less `.ts.net` origins, so a foreign `/` route is refused with the
  command to remove it.
- **`--json` waits for step 5,** where it carries the verification words.

**Tests:** `crates/cli/tests/setup.rs` (11), with a recording fake `System`.
- The generated config is parsed by the daemon's own loader (a `daemon`
  dev-dependency).
- Mutation-checked: removing the Funnel refusal, or the restore on a failed
  restart, fails a test.
- Not automated: real `tailscale serve`, `loginctl`, and the service manager.
  These are part of the VM acceptance.

### Step 4 — discovery (2026-10-12)

- **Contract:** `protocol::pairing::DiscoveryResponse` (`ProtocolRange`,
  `PairingSupport`), with fixture `tests/fixtures/pairing.json` and generated
  TS. `validate(probed_origin)` requires all of these:
  - `sentinal = "control"` and schema 1;
  - an origin equal to the probed origin;
  - a protocol range containing 1;
  - a ULID control ID, a strict release version, and a safe display name.

  Unknown fields are tolerated.
- **Daemon:** `control/discovery.rs` serves `GET /api/v1/discovery`.
  - Control only (a worker gets 404). No authentication.
  - `Cache-Control: no-store`; at most 1 KiB.
  - 60 answers per minute control-wide, not keyed on the spoofable header.
  - It names the control from `[control] display_name` (validated; falls back
    to the Machine name) and reports `approval: false` until step 5.
- **Client:** `crates/cli/src/discovery.rs`.
  - Candidates: online peers with a `.ts.net` name and a 100.64/10 or
    fd7a:115c:a1e0::/48 address. Capped at 64; 8 worker threads; 6 s run
    deadline. A probe already in flight still has its own 2 s limit.
  - The probe is `curl --resolve host:443:<peer ip>` (no rebinding),
    `--proto =https`, `--max-redirs 0`, a 4 KiB cap.
  - 404 is listed as "older Sentinal"; anything else unusable counts as
    "did not answer".
- **Setup:** `sentinal setup` option 2 lists the controls and never
  auto-picks; it falls back to a typed URL when none are found.

**Tests:**
- `protocol/tests/pairing.rs` (3);
- `daemon/tests/discovery_api.rs` (3: fields, fallback name, worker 404,
  rate limit);
- `tests/config.rs` (`display_name` validation);
- `cli/tests/discovery.rs` (3: filtering, address pinning, origin mismatch,
  404, bounds on count/concurrency/time, oversized/malformed). Mutation check:
  accepting an answer without the origin check fails a test.
- `cli/tests/setup.rs` (+3: list and pick, no auto-pick, none found).

### Step 5 — approval pairing (2026-10-13)

- **5a, protocol** (`6a488f1`):
  - request/created/proof/list/approval/claim/cancel types;
  - `MACHINE_ALREADY_LINKED`, `PAIRING_BUSY`, `INVALID_CLAIM`;
  - the 256-word list;
  - words = the first 4 bytes of SHA-256 over the NUL-separated
    `sentinal-pair-v1` transcript;
  - a shared vector in the fixture, computed independently in Python.
- **5b, store** (`4482c98`): migration **010** (008/009 were taken by 4.D.1),
  holding only the secret's hash and public nonces.
  - Approval requires a verified, unexpired PENDING row.
  - Claim requires the secret hash and an APPROVED, unclaimed, unexpired row.
    It writes the Machine and link in one transaction, using the registry/link
    writes now shared with code exchange; approval applies the owner's name.
- **5c, control** (`7b6d7e8`): `POST /api/v1/pairing/requests` verifies
  before answering.
  - The control calls the claimed endpoint back:
    `GET http://<addr>:<port>/api/v1/pairing/proof/<id>` must echo the request
    ID, the secret hash, and this control's ID.
  - Then `tailscale whois --json <addr>` must name the node at the same
    address.
  - Otherwise the request is cancelled and the reply is 403.
  - The owner lists, approves, and rejects. The worker claims (202/200/403/410,
    409 once claimed) or cancels.
  - Limits are control-wide: 5 pending, 20 per 10 min, and 20 wrong secrets
    cancel every open request.
- **5d, worker** (this commit):
  - The daemon (`link/approval.rs`) creates the secret and an unrelated
    control credential, and binds the first proof callback's request ID.
  - It refuses a reply naming a different control ID or request ID, and saves
    the link only on claim. The request is kept in memory only.
  - Local socket: `REQUEST_APPROVAL`, `CLAIM_APPROVAL`, `CANCEL_APPROVAL`.
  - `sentinal setup` sends a discovered control that advertises `approval`
    through `approval_command.rs` (words, 2 s claims, Ctrl-C cancels).
    Discovery now advertises `approval: true`.

**Deviations from §6:**
- **Migration number:** 010, not 008.
- **Pending requests are in memory on the worker:** a worker restart abandons
  them.
- **A mismatched control ID makes the control refuse the proof (403).** The
  worker therefore reports "could not verify (unreachable, or not the control
  that was discovered)" rather than a dedicated "different control" message.

**Tests:**
- `protocol/tests/pairing.rs` (6);
- `store/tests/pairing_requests.rs` (4; mutation-checked: dropping the
  `proof_ok` or the secret condition fails a test);
- `daemon/tests/approval_api.rs` (6; mutation-checked: an unchecked proof
  secret fails a test);
- `daemon/tests/approval_link.rs` (4, worker ↔ control over loopback HTTP
  with the real proof path);
- `cli/tests/approval_command.rs` (3);
- `cli/tests/setup.rs` (+1).

### Step 6 — approval UI (2026-10-13)

`PairingRequestsCard` sits at the top of Settings → Machines (desktop and
phone). It shows only verified PENDING/APPROVED requests:
- the claimed name and Tailscale's identity (node · login · OS · address);
- "Reconnects <name>" for a known inactive Machine;
- the four words in large monospace, the time left, and Approve/Reject per
  row. There is no "approve all".

It polls every 3 s and renders nothing on a worker or with nothing waiting.

**Tests:** `PairingRequestsCard.test.tsx` (4: per-row approve, reject and
reconnect text, empty/worker, phone). Playwright 13/13 still passes.

**Not automated:** an approval Playwright run needs `tailscale whois`, which
the e2e daemons do not have. It is part of the real-Machine acceptance.

### Step 7a — worker update coordination (2026-10-13)

- **Protocol** (`protocol::worker_update`):
  - `WorkerUpdateRequest {version, allow_interrupt}`;
  - `WorkerUpdateStatus {version, previous, state STARTED|INSTALLED|FAILED|UNKNOWN, reason, started_at}`;
  - the heartbeat request carries `update_status`; the heartbeat response and
    the registry carry `available_release`, and each registry Machine its
    `update`;
  - new failure code `LIVE_SESSIONS`. All additive.
- **Control** (`control/worker_update.rs`): `POST /api/v1/machines/{id}/update`
  (owner only).
  - It offers only the version in its own `channels/stable.json`
    (`ControlSettings.package_dir`).
  - It forwards the request to `POST http://<worker>/api/v1/link/update` with
    the control credential.
  - The worker's 409 `LIVE_SESSIONS` is passed through.
  - The latest report per worker is kept in memory and refreshed by every
    heartbeat. A STARTED with no report after 5 min shows as UNKNOWN.
  - Started, refused, installed, and failed updates are audited.
- **Worker** (`worker_update.rs`): `/api/v1/link/update` accepts only this
  link's control credential.
  - It refuses with live FULL Sessions unless `allow_interrupt`, and refuses
    while an update runs.
  - It records `worker-update.json` (0600), then starts
    `sentinal update --result-file …` detached: `systemd-run --user` on Linux,
    a new process group otherwise.
  - Status is derived after its own restart:
    - FAILED when the updater reported failure;
    - INSTALLED when the running version changed;
    - otherwise STARTED.
- **CLI:** `sentinal update` gains a hidden `--result-file`. The update itself
  is unchanged: install from the pinned control origin, verified, with the
  installer's own service-failure restore.
- **Web:** `WorkerUpdatesCard` (Settings → Machines) lists workers behind the
  stable release.
  - Each row has Update, with a confirmation naming the versions; a second
    confirmation appears for LIVE_SESSIONS.
  - It shows STARTED/FAILED (with the reason)/UNKNOWN.
  - It uses a bottom sheet on phones and is hidden when every worker is
    current. It shares `ActionCard.module.css` with the approval card.

**Deviations from §11:**
- **Health after the update:** the worker relies on the existing installer,
  which restores the previous release when the service fails to start. It
  does not add a separate 60 s heartbeat check.
- **Reported state names:** INSTALLED/FAILED, rather than
  STAGED/ROLLED_BACK.
- **No "update all workers in order"** yet; each worker is updated separately.

**Tests:**
- `daemon/tests/worker_update_api.rs` (2, over loopback HTTP with the real
  worker router):
  - unpublished version refused;
  - LIVE_SESSIONS, then consent;
  - the worker runs exactly `update --result-file …`;
  - the failure reaches the registry through a heartbeat;
  - only the control credential is accepted.

  Mutation check: removing the live-Session gate fails a test.
- `worker_update` status unit test;
- `protocol/tests/release.rs` (+1);
- `WorkerUpdatesCard.test.tsx` (4).
- Playwright 13/13.

### Step 7b — retention and the channel decision (2026-10-13)

- **Retention:** `publish-worker-release.sh` prunes after every publish. It
  keeps the newest `SENTINAL_PACKAGE_KEEP` releases (default 5), ordered by
  version, plus whatever `stable` points at. It deletes older
  `releases/<v>.{json,SHA256SUMS,SHA256SUMS.sig}` and
  `artifacts/sentinal-worker-<v>-*`. There is also a standalone
  `prune --package-dir DIR`. Test: `package-release-test.sh` (7 publishes →
  0.1.3–0.1.7; a rolled-back stable keeps its release under `KEEP=2`).
- **Beta (deviation from §10):** there is no separate worker `beta` pointer.
  The control's release policy already chooses stable or beta from GitHub
  (step 2), and whatever it receives is published to the one worker channel.
  Workers therefore follow the control's choice.
  - Why: a second worker pointer would need a channel-aware manifest, package
    route, bootstrap `install.sh`, and `sentinal update`, which is four
    formats for a single-owner pilot.
  - Revisit with fleets that need mixed channels.

### Step 8 — pilot runbook, backup, and preflight (2026-10-13)

- **`sentinal backup create [--output FILE]`:**
  - stops the user service, writes a 0600 `tar.gz` of the config and data
    directories, and always starts the service again;
  - refuses an output inside either directory or an existing file;
  - warns that the archive holds credentials.
- **`sentinal backup restore FILE`:**
  - only with the service stopped and an empty data/config directory (a lone
    `daemon.toml` is allowed);
  - refuses links, special files, `..`, absolute paths, and any entry outside
    the two directories;
  - resets both directories to 0700.
- **`sentinal doctor --pilot`** (read-only) adds:
  - listeners on 7420/7422 loopback-only (`lsof`, else `ss`);
  - exactly the two Serve routes, no raw TCP on 443, Funnel off (shared with
    setup);
  - HTTPS health, the dashboard, and `install.sh`;
  - the stable release;
  - the newest backup no older than 7 days.
- **`docs/PILOT.md`:** VM, install, setup, verify, workers and ACL, updates,
  backup, restore, and rebuilding without a backup.

**Not built (recorded in §8 and the recovery sequences):**
- `sentinal setup --reconnect` (moving a worker to a renamed control without
  pairing);
- a worker message for "a different control". Workers see their heartbeats
  refused, and the runbook says to disconnect and pair again.

**Tests:**
- `cli/tests/backup.rs` (4: stop/start order, 0600, entries, refused outputs,
  restore preconditions, hostile archive). Mutation-checked: dropping the
  entry filter fails a test.
- `cli/tests/doctor_pilot.rs` (2: a correct pilot; exposure + Funnel + dead
  HTTPS).

### Pilot findings from the v0.2.0 install (2026-10-13)

- **Release tools not executable.** The builder's final mode normalization
  (every file 644, then only the binaries and `install-worker.sh` back to 755)
  undid the tools' 755. On a control, `receive-release.sh` was "Permission
  denied", so `sentinal setup` could not publish workers' packages.
  - Fixed: the builder restores 755 on the three tools (test:
    `package-release-test.sh` checks the archive modes).
  - The CLI and `receive-release.sh` now invoke the tools through `bash`, so
    an already-installed 0.2.0 works too.
- **Provenance without gh login.** `setup` asked for GitHub provenance whenever
  `gh attestation verify --help` worked. It now also requires `gh auth status`;
  the owner signature is always required.
- **Owner password needed the Machine token** copied into the dashboard.
  `sentinal setup` (control) now asks for the password at a hidden prompt and
  sets it through `POST /api/v1/auth/setup` on loopback with the token it
  reads itself.
  - Nothing goes in argv.
  - Re-running setup asks until a password exists.
  - Without a terminal it is skipped, with a hint.
  - Tests: `cli/tests/setup.rs` (+3) and a raw-request unit test.
- **Doctor wording:** `Updates:` on a control names `sentinal release update`,
  and the pilot package error says how to publish.

### Control handoff: a control becomes a worker (2026-10-16)

`sentinal setup --role worker` on a control.

- **Protocol:**
  - `LinkMoveRequest {control_url, control_machine_id}`, control → worker
    `POST /api/v1/link/move` (control credential only);
  - `WorkerHandoff`;
  - local `HANDOFF_WORKERS {…, dry_run}` → `HANDOFF {workers}`.
- **Old control** (`control/handoff.rs`, reached only through the local
  socket):
  - validates the target and refuses itself;
  - sends the move to every active worker;
  - reports each one (not reachable, or an older release without the route);
  - audits `WORKER_HANDOFF`.
- **Worker** (`link/handoff.rs`):
  - asks the new control through the normal approval flow (callback proof +
    `whois` + owner approval), under its owner-chosen name;
  - keeps its current link until it claims the new one (claims every 2 s, gives
    up after 11 min);
  - the heartbeat loop then follows the new link.
- **CLI** (`setup/switch.rs`):
  - discovery, excluding this control's own origin;
  - preview listing the workers, then confirm;
  - handoff;
  - `worker_config` (removes `[control]`/`[packages]`, the Serve keys, and
    setup's comment; binds the Tailscale address; keeps other lines);
  - restart + health on the Tailscale address; restores the control config on
    failure;
  - `tailscale serve --https=443 off`;
  - then `Outcome::Approve` for this Machine.

**Tests:**
- `daemon/tests/handoff_api.rs` (2):
  - the full move over loopback HTTP: dry run, refusing self, the worker asks,
    still linked to the old control before approval, switched after;
  - only the control credential is accepted. Mutation-checked.
- `cli/tests/setup.rs` (+5): config round-trip, switch call order, decline,
  failed restart, no other control.
- `protocol/tests/pairing.rs` (+1).

**Owner approval is kept on purpose:** a control stepping down cannot attach
its workers anywhere without the new control's owner approving each one.

## Proving tests

- Installer/package: every target; fresh install, reinstall, upgrade, explicit
  rollback; corrupt or partial download; wrong signature, key, checksum,
  length, target, or protocol; unsafe permissions; paths with spaces;
  service-failure rollback; retained data; exact-confirmation uninstall.
  Fake `ssh-keygen` and service tools in fast tests, plus one real
  `ssh-keygen` round trip.
- Setup: each role, cancel at every prompt, rerun, role conflict, missing
  Tailscale, macOS app CLI, unsupported OS, `--yes`/`--json` flags, exit
  codes, Serve-conflict alternative port.
- Discovery: 0, 1, or many controls; duplicates; offline; ACL timeout;
  redirect; bad certificate; origin mismatch; LAN/public refusal; old version;
  oversized or malformed responses; ≤ 8 concurrent; no CIDR scan (an injected
  transport records every URL).
- Pairing: request, approve, reject, cancel, expire; one-success race; wrong
  secret; second claimant; proof callback failure; `whois` failure; claimed
  ACTIVE ID; word agreement (shared vectors); rate limits; owner auth; audit
  privacy; code fallback; fixtures; migration; generated TS.
- Update: availability, live-Session refusal, confirmation, Machine scope,
  independent verification, timeout, offline worker, health rollback, control
  restart, isolation.
- Control updates:
  - release watch:
    - only signed releases are offered;
    - draft, unsigned, and wrongly signed releases are refused;
    - channel filter, offline/rate-limit cache, and jitter (injected clock
      and HTTP);
  - update control: confirmation, staged swap through a fake service manager,
    health success and failure, rollback that restores the channel pointer;
  - worker version never ahead of the control;
  - **Update workers…** stops at the first failure;
  - automatic policy:
    - the window boundary;
    - skips a Machine with live FULL Sessions;
    - never downgrades or changes channel;
    - a rollback pauses it until acknowledged;
  - Push on each result.
- Web at 390×844 and 320×568: pending-approval list with words, approve and
  reject, update sheet. Playwright: one control plus two fake workers through
  discover → request → approve → heartbeat → rename → update one → reject one →
  revoke/re-pair.
- Security sweep: planted secrets absent from artifacts, logs, audits, argv,
  env, service files, browser storage, and URLs.

## Questions for the owner

1. **Release key custody:** approve an owner-held Ed25519 SSH release key
   (hardware key recommended) plus an offline backup key, with signing done
   locally after reviewing each draft release? The alternative is a key in
   GitHub Secrets, which gives CI signing authority (not recommended).
2. **Installers:** approve signed archives plus `install.sh` as V1's normal
   installer, deferring `.deb`, `.pkg` (needs a paid Apple ID), Homebrew, and
   apt? Or build `.deb` now, accepting a second, root-installed layout?
3. **Bootstrap URL:** use GitHub Releases
   (`…/releases/latest/download/install.sh`) for the first control, with a
   separate static landing page deferred?
4. **Identity proof:** require both the callback proof and `tailscale whois`
   before a request can be approved (recommended). Or allow approval with a
   warning when `whois` is unavailable?
5. **Live Sessions on update:** refuse by default and require an explicit
   "interrupt N Sessions" confirmation (recommended), or always refuse until
   Reliability R1?
6. **Control update policy:** default **Notify** (one button). Offer
   **Automatic when idle** as opt-in with a 03:00–05:00 window. May the
   control contact api.github.com daily for this?
7. **Pilot acceptance:** reuse the existing `sentinal-pilot` VM, wiped to a
   clean state, as the 4.E acceptance control?

## Real-Machine pilot matrix (to fill during acceptance)

| Role | Machine | OS / arch | Install path | Result |
|---|---|---|---|---|
| Control | Cloud VM (Ubuntu 24.04) | Linux x86_64 | GitHub `install.sh` → `sentinal setup` (control) | pending |
| Worker | MacBook Pro | macOS arm64 | control `install.sh` → `sentinal setup` (worker) | pending |
| Worker | pop-os | Pop!_OS x86_64 | same | pending (offline since 2026-10-09) |
| Worker | Mac mini | macOS arm64 | same, after leaving its own control role | optional |
| Phone | iPhone 13 Pro | iOS PWA | approve, Push, open Session | pending |

## Release and signing evidence

_To be recorded: signed SHA256SUMS, pinned key fingerprints (public only),
provenance verification output, and honest unsigned/unnotarized coverage._
