# Deployment check — cloud VM control, home Tailnet, small-team pilot

Status: **IN PROGRESS 2026-10-10.** P0 and P1 are done; owner setup is next. This is a baseline taken *before* Milestone 4.E. It uses
the flow that exists today: curl installer, `sentinal connect` with a one-time code,
and Tailscale Serve configured by hand. Every manual step and every failure it
records is input to the 4.E design review. It is not V1 acceptance; release
blockers F1, F5, F9, and F10 in `docs/AUDIT_TRACKER.md` are still open.

## Candidate

| Item | Value |
|---|---|
| Commit | `168692c` (Milestone 4.F committed) |
| Release | `0.1.14` |
| `aarch64-apple-darwin` | built on the Mac mini, 8 655 810 bytes, sha256 `53173bdf…c56ed` |
| `x86_64-unknown-linux-musl` | to build on the VM (no Linux toolchain or Docker on the Mac mini) |
| Signing | none: unsigned and unnotarized (the 4.E trust root does not exist yet) |

## Topology

```text
                 Tailnet  <owner tailnet> (MagicDNS + HTTPS)
  iPhone / browsers ──443──▶  sentinal-pilot  (cloud VM, CONTROL)
                               sentinald 127.0.0.1:7420  (API)
                               sentinald 127.0.0.1:7422  (PWA + packages)
                               tailscale serve  / → 7422, /api → 7420
                     ◀──7420── workers: MacBook, pop-os, Linux worker VM, (Mac mini)
  Public Internet ─────────▶  VM public IP: SSH only (or nothing), no Sentinal port
```

The Mac mini stays as the current control and is not changed unless the owner
chooses to move it under the pilot. Moving a worker means `sentinal disconnect`
followed by `sentinal connect <pilot URL>`. Its Machine ID is kept, and it can
reconnect to the Mac mini the same way afterwards.

## Phases and checks

### P0 — VM preparation (owner)
- Ubuntu 24.04 LTS, 2 vCPU, 4 GB RAM, 40 GB disk; a second, smaller VM as a Linux worker if possible.
- Provider firewall: inbound SSH 22 from the owner's IP only (or nothing,
  using Tailscale SSH); everything else denied.
- `sudo tailscale up --hostname sentinal-pilot`, run interactively by the owner.
  No auth key is committed or pasted into chat.
- Add the Mac mini's SSH public key for a non-root user with sudo.

### P1 — Control install on the VM
1. Install the build prerequisites, copy the exact commit with `git archive 168692c`
   (no unrelated local files), then `cargo fetch --locked`.
2. Build the `x86_64-unknown-linux-musl` artifact on the VM. Verify its
   BUILD.json commit, size, and SHA-256.
3. Install from the release directory with `--bind 127.0.0.1:7420`. Enable
   linger. Write `daemon.toml` with `[control]`, `[packages]`,
   `behind_tailscale_serve`, and `allowed_origins`, as described in `docs/SETUP.md`.
4. Publish 0.1.14 (Linux + macOS) to the VM package directory.
5. Configure Serve: `/` → 7422 and `/api` → 7420. Never enable Funnel.
6. Set the owner password with the bootstrap token in a private terminal. Do
   not print the token.

### P2 — Fleet (3–5 workers)
- MacBook (macOS arm64), pop-os (Pop!_OS x86_64, once it is back online), and the Linux
  worker VM. The Mac mini joins only if the owner chooses.
- On each worker: curl `install.sh` from the pilot URL, then `sentinal connect`
  with a code from Settings → Machines, then confirm `sentinal connection status` = 0.
- On each worker: Watch success, failure, and waiting jobs. Then check, from
  the PWA and the iPhone:
  - each Session shows up under the right Machine (namespaced identity);
  - push notifications arrive;
  - tapping a notification opens that exact Session.
- Publish 0.1.15 (a no-op rebuild), update **one** worker, run with mixed
  versions, then `rollback-worker` and `sentinal update --rollback 0.1.14`.
- Revoke one worker. Confirm the others are unaffected, then re-pair it and
  confirm it keeps the same Machine ID.

### P3 — Tailnet ACL lockdown (owner applies in the Tailscale admin console)
```json
"tagOwners": {
  "tag:sentinal-control": ["autogroup:owner"],
  "tag:sentinal-worker":  ["autogroup:owner"]
},
"grants": [
  {"src": ["autogroup:owner"],    "dst": ["tag:sentinal-control"], "ip": ["443"]},
  {"src": ["tag:sentinal-worker"], "dst": ["tag:sentinal-control"], "ip": ["443"]},
  {"src": ["tag:sentinal-control"], "dst": ["tag:sentinal-worker"], "ip": ["7420"]}
]
```
Keep the existing owner-to-own-devices grant (SSH, other services) during the
test, and use the policy file's `tests` block to assert the denials before saving.
Negative checks:
- worker → worker:7420 refused;
- worker → control:7420/7422 refused (loopback only);
- owner → worker:7420 refused;
- a non-owner device → control:443 refused.

### P4 — Security sweep
- From outside the Tailnet: TCP scan of the VM's public IP (all ports). Expect
  SSH only, or nothing. The scan must not reach 443, 7420, or 7422.
- On the VM: `ss -ltnp`, which must show only loopback for 7420/7422; `tailscale serve status`
  and `tailscale funnel status`, which must show Funnel off; ufw/nftables rules.
- Files: modes of `~/.config/terminal-agent/*` (0600) and their directories (0700).
  Rendered service files contain no token.
- Leaks: plant a unique canary password, pairing code, and terminal string.
  Then grep for each in:
  - the journal, `audit.jsonl`, `package-audit.jsonl` and the artifacts;
  - service files, `/proc/*/cmdline` and the environment;
  - the PWA's browser storage and Cache Storage.
  None of the three may appear anywhere.
- Login throttling: repeated wrong passwords lock out. Record F10 (rotating
  `Tailscale-User-Login` from a local process) as known and still open.

### P5 — Load / soak (≥ 6 h)
- Per worker: 20 concurrent watched Sessions printing 1 line/s, plus 5 that
  each produce 50 MB of output. Every 10 min, a Session ends, alternating success and failure.
- Every 5 min, record:
  - `sentinald` RSS and CPU on every Machine;
  - size of the data dir and of the SQLite file;
  - push latency, from Event timestamp to the delivery being accepted.
- Restart the control service mid-soak. Workers must reconnect and Push must
  resume; the live Sessions on the control's own Machine are interrupted (F1, expected).
- Pass criteria:
  - no unbounded RSS growth (< 10 % drift after the first hour);
  - disk growth matches the output volume, with F9's missing limits recorded;
  - zero lost Events;
  - Push p95 under 10 s.

### P6 — Backup and rebuild
- Stop the service, then tar `~/.config/terminal-agent` and
  `~/.local/share/terminal-agent` with permissions preserved, and copy the archive off the VM.
- Rebuild the control from the backup on a fresh VM, or in place, using
  disposable pilot data only. Workers reconnect without a new pairing, and existing Push
  subscriptions keep working, because the VAPID key is restored.

## What this test cannot claim
- It is a single-owner pilot: one password, with no users, roles, SSO, tenant isolation, high availability, or SLA.
- Artifacts are unsigned, so trust rests on the Tailnet HTTPS origin plus a checksum served by the same origin.
- A daemon restart kills live FULL Sessions until Reliability R1.

## Evidence

_Sanitized; no secrets, public IPs, or auth keys._

### P0/P1 — 2026-10-10, VM `sentinal-pilot` (Ubuntu, x86_64, Tailnet member)

The owner ran every command on the VM; the assistant had no shell access to it.
The source bundle for `168692c` (with the PWA prebuilt) and the macOS artifact
were sent from the Mac mini with Taildrop.

- Linux build on the VM: `Finished release profile in 25m 52s`.
- Installed with the release's own `scripts/install-worker.sh --release-dir`. Then:
  - `daemon.toml` was edited by hand to bind loopback and add `[control]`/`[packages]`;
  - 0.1.14 was published for both targets;
  - Serve was configured as `/` → 7422 and `/api` → 7420.
- On the VM: `sentinald` active; `127.0.0.1:7420` and `127.0.0.1:7422` only; Serve
  "tailnet only"; Funnel not listed.
- From the Mac mini over the Tailnet:
  - health `{"ok":true,"version":1}`;
  - PWA 200 with CSP and `X-Frame-Options: DENY`;
  - `/install.sh` 200;
  - channel stable 0.1.14 `[x86_64-unknown-linux-musl, aarch64-apple-darwin]`;
  - `auth/status` `{"setup_required":true}`;
  - TCP 7420/7422/80 closed, 22/443 open.

### Findings for the 4.E design (manual-flow friction)

1. **Bug:** `build-worker-release.sh` rejects its own freshly built binaries
   when the umask is `0002` (Ubuntu default with user-private groups): "unsafe
   source binary … must be … not group/world writable". It should normalise the
   modes of the binaries it builds itself.
2. Compiling on a small VM took ~26 min; a normal install must download a
   prebuilt, verified release.
3. The installer accepts only a Tailscale IPv4 `--bind`. A control must be hand-edited to
   loopback and given `[control]`/`[packages]` (17 lines of TOML).
4. Nothing enforces step order. After the build failed, the later steps ran,
   found nothing to install, and failed in confusing ways ("Unit not found",
   "missing shell assets").
5. Linger, Serve routes, and publishing are separate manual commands. Together
   with item 3, these are the `sentinal setup` CONTROL preview/apply scope.
6. The first control's installer has no trust root of its own; this run
   relied on Taildrop between the owner's own devices.
7. Moving a worker to another control (`disconnect` then `connect`) leaves its
   **update origin** on the old control. The MacBook's `release.json` still said
   `control_url: <mac mini>`, so `update --check` reported 0.1.13 as current. Re-running
   the new control's `install.sh` moved the origin. Pairing and update origin
   should move together, or `doctor` should flag the mismatch.
8. **Bug:** the served `install.sh` always prints `State: UNPAIRED` and a
   connect hint, even when it is updating a paired worker. The link was in fact
   preserved (`connection status` = connected, same Machine ID).
9. Run from a real terminal, success and failure Events worked: the failure
   Session recorded `COMMAND_FINISHED exit 7`, and the owner received both
   notifications. The earlier empty Events came from the `script -q` workaround
   over SSH, not from the product.
10. **Bug, fixed in the working tree:** the waiting test from `docs/SETUP.md`
    (`printf "Continue? [y/N] "; read answer`) never produced
    `WAITING_FOR_INPUT`. The Session stayed RUNNING. The generic prompt rule only
    trimmed `\r\n`, so the trailing space defeated every suffix match; the existing test
    used `"Continue? [y/N]"` with no space. Fix: trim trailing whitespace before
    matching. A bare `?` counts only when whitespace follows it. New test
    `prompts_with_the_usual_trailing_space_still_wait`.
11. **Stalled-notification spam on 0.1.14:** MacBook Sessions 7, 8, and 11 each
    sent `POSSIBLY_STALLED` exactly 10 min after their last Command *finished*.
    These were idle shells, so the alerts were false. Sessions 9 and 10 were in fact waiting at
    a prompt (finding 10). The idle-shell gate (`with_command_activity_required`)
    and once-per-Event presentation landed on `main` in `0737f98` and `511daff`,
    after the `168692c` candidate. 0.1.14 did not contain them. Re-test on 0.1.15.
12. The candidate commit moved under the test: four commits landed on `main`
    during the run. A release must name one exact tag, built by an automated pipeline.

### Release pipeline decision (2026-10-10)

The owner chose GitHub Actions for building releases; the repository is public.
1. A `v*` tag builds every target on native Linux and macOS runners.
2. The workflow attaches SHA256SUMS and GitHub build-provenance attestations to a
   draft Release.
3. The VM downloads the release, verifies the attestations and hashes, and
   publishes it to its Tailnet package channel.
4. Workers keep updating from the VM only.

Apple signing and notarization stay out of scope (no Apple Developer account).

### P2 — Fleet (in progress)

- The MacBook moved from the Mac mini to the pilot with `disconnect` and a new one-time
  code. `connection status`: connected, same Machine ID, display
  name "MacBook".
- MacBook updated 0.1.13 → 0.1.14 from the pilot's `install.sh` (review-first
  form). Installed sha256 `53173bdf…` matches the build. No live Sessions
  were interrupted (only old interrupted records existed).

### Fixed for 0.1.16 (from the pilot)

| Finding | Fix |
|---|---|
| 1 Packager rejects its own binaries under umask 0002 | Builder normalises modes of binaries it compiled; supplied binaries still refused |
| 4/5/8 Manual publish steps, "UNPAIRED" after every install | `just receive-release`: download, verify SHA256SUMS + BUILD.json + GitHub provenance, publish, update the control. `install.sh` prints four steps, a progress bar, and the real link state. |
| `doctor` always printed `Pairing: UNPAIRED` | Real role, link state (never credentials), version, and update origin |
| `doctor` could not find Tailscale on macOS | PATH, then the app-bundle CLI; `SENTINAL_TAILSCALE_CLI` overrides |
| `sentinal 0.0.1` everywhere | Release builds stamp `SENTINAL_RELEASE_VERSION`/`SENTINAL_GIT_COMMIT`: `sentinal 0.1.16 (abc1234)`; the daemon reports the release to the registry |
| VM `update --check`: "missing control_url" | Clear no-origin error that points the control to `just receive-release` |
| `sentinal: command not found` after install | Installer and `install.sh` print a PATH hint |
| Ubuntu `gh` 2.45 lacks `attestation` | `receive-release` refuses without gh 2.49+ unless `--skip-provenance` is explicit; since 4.E step 1 the owner signature is always required |
