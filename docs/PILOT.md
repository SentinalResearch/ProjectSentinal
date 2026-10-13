# Private pilot runbook

One owner, one control on a small Tailnet VM, a few workers. Nothing is
reachable from the internet: the control listens on loopback only, and
Tailscale Serve publishes it to your Tailnet over HTTPS. This is a private,
single-tenant pilot. It has no SSO, multi-user accounts, high availability,
or public ingress (see `docs/progress/milestone-4e.md` §13).

Known limit: restarting `sentinald` (including during an update) interrupts
live FULL Sessions (audit finding F1, until Reliability R1).

## 1. The VM

- Ubuntu 24.04, 2 vCPU, 4 GB RAM, 40 GB disk.
- Provider firewall: no inbound ports, or SSH only from your own address.
  Sentinal needs no public port.
- Install Tailscale, run `sudo tailscale up`, and enable MagicDNS and HTTPS
  certificates for the Tailnet in the admin console.
- Let your user configure Serve without `sudo`:
  `sudo tailscale set --operator=$USER`.

## 2. Install the control

Download the installer, the signed checksums, and the pinned release key into
an empty directory:

```console
vm$ mkdir -p ~/sentinal-install && cd ~/sentinal-install
vm$ base=https://github.com/SentinalResearch/ProjectSentinal/releases/latest/download
vm$ curl -fsSLO "$base/install.sh"
vm$ curl -fsSLO "$base/SHA256SUMS"
vm$ curl -fsSLO "$base/SHA256SUMS.sig"
vm$ curl -fsSL -o allowed_signers \
      https://raw.githubusercontent.com/SentinalResearch/ProjectSentinal/main/packaging/release-keys/allowed_signers
```

For a specific release, use `base=https://github.com/SentinalResearch/ProjectSentinal/releases/download/v0.1.19`
instead of `latest`.

Check the owner's signature, then that `install.sh` is the signed file:

```console
vm$ ssh-keygen -Y verify -f allowed_signers -I sentinal-release -n sentinal-release \
      -s SHA256SUMS.sig < SHA256SUMS
Good "sentinal-release" signature for sentinal-release with ED25519 key SHA256:…
vm$ grep ' install.sh$' SHA256SUMS | sha256sum -c -
install.sh: OK
```

Both must succeed; if either fails, stop and do not run the script. Then
install:

```console
vm$ sh install.sh
```

The installer downloads the archive for this Machine. It refuses any archive
whose checksum is not in the owner-signed `SHA256SUMS` from a pinned key. It
installs `sentinal` and `sentinald` into `~/.local/bin` and starts the user
service. If `~/.local/bin` is not on `PATH`, it prints the line to add.

On macOS, use `shasum -a 256 -c -` in place of `sha256sum -c -`.

## 3. Make it the control

```console
vm$ sentinal setup            # 1) Create my Sentinal control
```

Setup previews every change, then applies it:

- `daemon.toml` (loopback bind, `[control]`, `[packages]`);
- linger, so the service survives logout;
- the service restart and health check;
- the Serve routes `/` → `:7422` and `/api` → `:7420/api`;
- publishing the installed release for workers.

It never turns on Funnel and never replaces a Serve route it did not create.
Re-run it any time; on a finished control it prints "Nothing to do".

Last, setup asks for the dashboard's owner password (twice, hidden). There is
no token to find or copy: setup reads the Machine token from
`~/.config/terminal-agent/token` itself and sends both only to the daemon on
`127.0.0.1`. Then open `https://<vm>.<tailnet>.ts.net` on a Tailnet device and
sign in. If you skip the prompt, re-run `sentinal setup` to set it later.

## 4. Verify

```console
vm$ sentinal doctor --pilot
Pilot listeners: OK (7420 and 7422 on loopback only)
Pilot Serve: OK (/ and /api on 443; Funnel off)
Pilot HTTPS: OK (health, dashboard, install.sh)
Pilot packages: OK (stable 0.1.19)
Pilot backup: OK (newest is less than a day old)
```

`doctor --pilot` is read-only. Any `ERROR` line names the fix. Its `Storage:`
line shows Session-log, audit, and database use against the `[storage]`
limits.

## 5. Workers

On each worker:

```console
worker$ curl -fsSL https://<vm>.<tailnet>.ts.net/install.sh | sh
worker$ sentinal setup        # 2) Connect to my existing Sentinal
```

Pick the control from the list. The worker prints four words. In Settings →
Machines, approve the waiting row whose words match. A worker that cannot be
verified over Tailscale is never shown as approvable.

Tailnet ACL (grants), so only the intended devices reach Sentinal:

```json
"tagOwners": {
  "tag:sentinal-control": ["autogroup:owner"],
  "tag:sentinal-worker":  ["autogroup:owner"]
},
"grants": [
  {"src": ["autogroup:owner"],      "dst": ["tag:sentinal-control"], "ip": ["443"]},
  {"src": ["tag:sentinal-worker"],  "dst": ["tag:sentinal-control"], "ip": ["443"]},
  {"src": ["tag:sentinal-control"], "dst": ["tag:sentinal-worker"],  "ip": ["7420"]}
]
```

## 6. Updates

- **The control:** Settings → Machines → *Control release* shows new signed
  releases. Press **Update control**. A failed health check rolls back
  automatically. The CLI equivalent is `sentinal release update`.
- **Workers:** after the control updates, *Worker updates* lists each worker
  behind it. Press **Update** on one Machine at a time. A worker with live
  FULL Sessions asks for a second confirmation.
- **Releases kept:** the newest 5 releases stay published, so
  `sentinal release rollback <version>` can go back that far.

## 7. Back up

```console
vm$ sentinal backup create
```

It stops `sentinald`, writes an owner-only (0600)
`~/sentinal-backups/sentinal-backup-<time>.tar.gz` of the config and data
directories, and starts the service again.

The archive holds the Machine ID, worker credentials, the password hash, the
Push key, SQLite, and terminal output. Copy it somewhere private, never to
shared storage. Take one at least weekly; `doctor --pilot` reports one older
than 7 days as an error.

## 8. Restore onto a rebuilt VM

1. Provision a VM (§1) and install (§2). Do not run setup yet.
2. Stop the service and restore:

   ```console
   vm$ systemctl --user stop sentinald
   vm$ sentinal backup restore sentinal-backup-<time>.tar.gz
   vm$ systemctl --user start sentinald
   vm$ sentinal setup        # adds linger and the Serve routes again
   ```

Restore refuses a running service, a non-empty data directory, and any
archive entry outside the two Sentinal directories.

Workers reconnect on their own with the same Machine ID and credentials, as
long as the VM keeps its MagicDNS name. Give the rebuilt VM the old name. If
the name changes, each worker must pair again: `sentinal disconnect`, then
`sentinal setup`. Its Machine ID is kept; a `--reconnect` mode that moves a
worker to a new name without pairing is not built yet.

## 9. Without a backup

The rebuilt control is a new control with a new ID and no record of the
workers, so it refuses their heartbeats. On each worker run
`sentinal disconnect`, then `sentinal setup`, and approve it again. Each phone
must also subscribe to Push again.
