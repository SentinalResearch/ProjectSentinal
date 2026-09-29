# Setup and two-Machine acceptance

Milestone 1c supports one selected worker Machine and one control Machine.
The worker runs `sentinald`; the control Machine runs only the web UI and
proxies `/api` to that worker over Tailscale. This is not a central control
plane and there is no Machine enrollment or aggregation yet.

## Prerequisites and Tailnet policy

Both Machines need:

- an active Tailscale connection in the same Tailnet;
- a Tailscale IPv4 address in `100.64.0.0/10`;
- a Tailnet ACL and host firewall that permit the control Machine to reach
  worker TCP port `7420`, and test devices to reach control TCP port `5173`;
- Rust stable and cached Cargo dependencies to build the worker from source;
- Node.js and `pnpm` on the control Machine to run the development UI.

Check each address with `tailscale ip -4` and connectivity with
`tailscale ping <worker-name-or-IP>`. Do not open the daemon or Vite ports to
an ordinary LAN or the public internet. Sentinal rejects wildcard, non-
Tailscale, and malformed addresses; it never silently falls back to one.

## Install a worker

From a checked-out source tree on the worker:

```console
$ just install-worker --bind 100.x.y.z
```

With no `--bind`, the installer asks Tailscale for this Machine's address.
It builds offline with Cargo's cached dependencies, installs `sentinal` and
`sentinald` under `~/.local/bin`, copies the approved shell-hook assets, writes
`~/.config/terminal-agent/daemon.toml` only when absent, and enables the
native user service. Add `~/.local/bin` to `PATH` if necessary.

Preview without building, writing, or invoking a service manager:

```console
$ just install-worker --bind 100.x.y.z --dry-run
```

For a prebuilt release directory containing `bin/sentinal`,
`bin/sentinald`, and `shell/`, run:

```console
$ scripts/install-worker.sh install --release-dir /absolute/release --bind 100.x.y.z
```

`--prefix /absolute/path` relocates program and rendered service files;
`--no-service` installs without touching launchd or systemd. Source binaries
must be regular executable files owned by the current user and not writable
by a group or others.

Default installed paths:

| Item | macOS and Linux |
|---|---|
| CLI and daemon | `~/.local/bin/sentinal`, `~/.local/bin/sentinald` |
| Config/token/password hash | `~/.config/terminal-agent/` |
| SQLite/logs/audit/runtime | `~/.local/share/terminal-agent/` |
| Shell-hook package assets | `~/.local/share/sentinal/shell/` |
| macOS service | `~/Library/LaunchAgents/dev.sentinal.sentinald.plist` |
| Linux service | `~/.config/systemd/user/sentinald.service` |

## First start and authentication

The daemon creates an owner-only Machine token on first start. There is no
default password. On the control Machine, start the UI as described below,
open its displayed URL, and use the setup page to submit the Machine token
and choose the single owner's password. Later browser visits use that
password.

Read the bootstrap token only in a private worker terminal:

```console
$ cat ~/.config/terminal-agent/token
```

Do not print or copy the token or password hash into logs, screenshots, bug
reports, or shared shell history. The password plaintext is never stored;
`auth.json` contains an Argon2id hash.

An installed `sentinal` with no explicit `--url` reads the bind from
`~/.config/terminal-agent/daemon.toml`, so local worker commands reach the
Tailscale-bound daemon. `--url` and `SENTINAL_URL` remain explicit overrides.

## Development launch roles

For a combined same-Machine development setup:

```console
$ just network
```

For a two-Machine test, run only the daemon on the worker:

```console
worker$ just agent
```

This source-tree role uses a temporary daemon config. In another worker
terminal, point the CLI at the printed daemon URL explicitly:

```console
worker$ SENTINAL_URL=ws://100.x.y.z:7420/api/v1/ws \
  sentinal watch -t "Handoff test" -- python3 -u scripts/examples/long_job.py
```

On the control Machine, select exactly that worker:

```console
control$ just control WORKER_IP=100.x.y.z
```

`just control WORKER_IP=...` starts no local daemon and always forces
`VITE_DATA=api`, even if the caller exported `VITE_DATA=mock`. It prints the
control URL and selected worker. None of these commands falls back to
fixtures or localhost. Use `--dry-run` to inspect each role.

## Service operations

Linux user service:

```console
$ systemctl --user status sentinald.service
$ journalctl --user -u sentinald.service --since today
$ systemctl --user restart sentinald.service
```

macOS user agent:

```console
$ launchctl print "gui/$(id -u)/dev.sentinal.sentinald"
$ tail -F ~/.local/share/terminal-agent/sentinald.stderr.log
$ launchctl kickstart -k "gui/$(id -u)/dev.sentinal.sentinald"
```

The installer renders service files with actual installed paths. It never
embeds the Machine token or password hash. The files under `packaging/` are
reference templates, not substitutes for installer rendering.

## Upgrade

Re-run the install command from the new checkout or release directory.
Binaries are atomically replaced where the platform permits. Existing
`daemon.toml`, token, password hash, SQLite database, Session logs, adapters,
and recipes are preserved.

```console
$ just install-worker --bind 100.x.y.z
```

The user service restarts. FULL Sessions are interrupted by a `sentinald`
restart and cannot be reattached until Reliability R1 implements a separate
per-Session owner. Their saved output remains available.

## Backup and inspection

Stop or quiesce the daemon before a filesystem-level backup, then copy both
`~/.config/terminal-agent/` and `~/.local/share/terminal-agent/` while
preserving permissions. The first contains credentials; the second contains
the database and potentially sensitive terminal output.

Useful metadata-only checks:

```console
$ curl --fail http://100.x.y.z:7420/api/v1/health
$ sqlite3 ~/.local/share/terminal-agent/terminal-agent.sqlite3 '.tables'
$ find ~/.local/share/terminal-agent/sessions -type f -name '*.log.zst' -print
$ jq -c '{timestamp,client,session_id,byte_count}' \
    ~/.local/share/terminal-agent/audit.jsonl
```

The remote-input audit is metadata only: time, client, Session ID, and byte
count, never the input bytes. Do not dump the SQLite rows or compressed
Session output in routine diagnostics. Use the bounded authenticated replay
API from `docs/CLI.md` when output inspection is required.

Inspect the local ingress without revealing request payloads:

```console
mac$ stat -f '%Sp %Su %N' ~/.local/share/terminal-agent/run \
  ~/.local/share/terminal-agent/run/sentinald.sock
linux$ stat -c '%A %U %n' "$XDG_RUNTIME_DIR/sentinal" \
  "$XDG_RUNTIME_DIR/sentinal/sentinald.sock"
```

## Uninstall

Programs and the user service are removed while config, credentials, SQLite,
logs, adapters, and recipes remain:

```console
$ just uninstall-worker
```

Preview with `just uninstall-worker --dry-run`. Data deletion is a separate,
exact-path confirmation:

```console
$ just uninstall-worker \
  --data-dir "$HOME/.local/share/terminal-agent" \
  --delete-data "$HOME/.local/share/terminal-agent"
```

The installer displays the exact preserved or deleted paths. It never removes
config when deleting data.

## Two-host acceptance checklist

Record both OS versions, both Tailscale IPv4 addresses, the commit, and every
command in `docs/progress/milestone-1c.md`.

1. On the worker, install/start `sentinald`; verify its public health route
   from the control Machine.
2. On the control Machine, run `just control WORKER_IP=<worker-IP>`, set the
   owner password with the private bootstrap token, and confirm no `MOCK`
   badge or fixture Sessions appear.
3. On the worker, start
   `sentinal watch -t "Handoff test" -- python3 -u scripts/examples/long_job.py`.
4. Detach with `Ctrl-\`, then `d`, or close that terminal. Confirm the process
   and Session remain live in the browser.
5. Attach in the browser, send safe input to the shell after the job, detach
   the pane, and reattach with `sentinal attach <ID>` on the worker.
6. Let one Session exit gracefully (`exit 0`) and confirm it becomes
   `COMPLETED`, read-only, and replayable. Create another and use Kill; confirm
   `INTERRUPTED` without a raw OS `EIO`.
7. Inspect `terminal-agent.sqlite3`, the matching `sessions/<id>.log.zst`, and
   `audit.jsonl`. Confirm Command/Session state and output exist, and the audit
   contains byte counts but none of the test input payload.
8. Restart the service and verify finished output still replays. Record that
   any live FULL Session was interrupted, as expected before Reliability R1.

## Current boundaries

- The control UI targets one configured worker. There is no Machine
  enrollment or aggregation, heartbeat routing, pairing flow, or central
  server.
- The worker package is exactly `sentinald`, `sentinal`, and shell assets.
  There is no separate watcher-agent process; Milestone 3's watcher will be a
  deterministic library linked into `sentinald`.
- Tailscale supplies private networking and access policy. Sentinal does not
  configure the Tailnet or bypass host firewalls.
- Browser/CLI disconnects do not end a Session. A daemon restart is different:
  FULL Sessions are interrupted and their PTYs are not reattachable yet.
- Rich watcher-derived progress, health, errors, and adopted-log tailing remain
  later milestones. Real mode shows only data the current daemon supplies.
