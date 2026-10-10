# Setup and two-Machine acceptance

Workers run `sentinald`. Since Milestone 4.0 one owner-selected control
Machine also keeps a registry of enrolled workers: each worker is connected
explicitly with `sentinal connect` and a one-time code, and gets a permanent
Machine ID, an editable display name, and its own revocable credential. This
is not a cloud service. Milestone 4.A aggregation routes each browser action
through the control daemon to the worker that owns the selected Session; the
browser authenticates only to the control origin.

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
| Production PWA assets | `~/.local/share/sentinal/web/` |
| macOS service | `~/Library/LaunchAgents/dev.sentinal.sentinald.plist` |
| Linux service | `~/.config/systemd/user/sentinald.service` |

## Private Tailnet package delivery

Milestone 1c.1 supports one owner-selected control Machine serving verified
worker releases. The package role is a second, loopback-only HTTP listener in
the existing `sentinald`; it is not another agent or a public cloud service.
Tailscale Serve terminates Tailnet HTTPS and proxies to the listener. Never
enable Funnel for this service.

The control Machine needs `cargo`, `tar`, `gzip`, a SHA-256 tool
(`sha256sum` or `shasum`), Tailscale with MagicDNS/HTTPS enabled, and a
Tailnet Grant allowing only the intended owner/worker identities to reach
control TCP port 443. Build each target on a toolchain that actually supports
it; missing cross-target toolchains fail rather than producing a false build:

```console
control$ just package-worker 0.0.1 aarch64-apple-darwin dist
control$ just package-worker 0.0.1 x86_64-unknown-linux-musl dist-linux
```

Supported targets are `x86_64-unknown-linux-musl`,
`aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, and
`aarch64-apple-darwin`. Publish every artifact for one version together:

```console
control$ scripts/publish-worker-release.sh publish \
  --version 0.0.1 \
  --package-dir "$HOME/.local/share/terminal-agent/packages" \
  --artifact "$PWD/dist/sentinal-worker-0.0.1-aarch64-apple-darwin.tar.gz" \
  --artifact "$PWD/dist-linux/sentinal-worker-0.0.1-x86_64-unknown-linux-musl.tar.gz"
```

The publisher validates target, size, and SHA-256; installs immutable files
first; and atomically moves `stable` last. Reusing a version with different
bytes is refused. An interruption cannot partially advance stable.

Enable the package role in the control Machine's existing
`~/.config/terminal-agent/daemon.toml` using real absolute values:

```toml
[packages]
enabled = true
listen = "127.0.0.1:7422"
directory = "/Users/OWNER/.local/share/terminal-agent/packages"
public_base_url = "https://sentinal-control.TAILNET.ts.net"
max_artifact_bytes = 268435456
web_root = "/Users/OWNER/.local/share/sentinal/web"
```

The daemon rejects plain HTTP, URL paths/ports/credentials, non-`.ts.net`
names, and a package listener that is not loopback. Restart `sentinald`, then:

```console
control$ tailscale serve --bg --https=443 http://127.0.0.1:7422
control$ tailscale serve status
```

Use the reviewable installation form first:

```console
worker$ curl --proto '=https' --tlsv1.2 --fail --silent --show-error \
  --output install-sentinal.sh \
  https://sentinal-control.TAILNET.ts.net/install.sh
worker$ less install-sentinal.sh
worker$ sh install-sentinal.sh
```

The convenience form is:

```console
worker$ curl -fsSL https://sentinal-control.TAILNET.ts.net/install.sh | sh
```

The bootstrap selects an exact OS/architecture, disables redirects, checks
the declared compressed length and SHA-256, refuses unsafe archive entries,
and only then calls the offline installer. It starts the user service and
ends with:

```text
Sentinal worker installed and running
State: UNPAIRED
Next: create a code in the control under Settings → Machines → Add Machine, then run:
  sentinal connect https://sentinal-control.TAILNET.ts.net
```

Installation never pairs: no code or credential is in the script, the
package, the service file, or the update path. Enroll the worker explicitly
(see Enroll a worker). Inspect and update without pairing:

```console
worker$ sentinal doctor
worker$ sentinal update --check
worker$ sentinal update
```

An accidental downgrade is refused. For an explicit rollback, the control
owner moves stable to an existing immutable release, then the worker confirms
that exact version:

```console
control$ just rollback-worker 0.0.1 \
  "$HOME/.local/share/terminal-agent/packages"
worker$ sentinal update --rollback 0.0.1
```

Reinstall/update preserves configuration, credentials, SQLite, Session logs,
audits, adapters, and recipes. A service-start failure restores the previous
program bundle. A daemon restart still interrupts live FULL Sessions until
Reliability R1.

Package manifests/scripts are capped at 64 KiB, artifacts at 256 MiB, and
eight artifact streams may run concurrently. Ranges and arbitrary paths are
refused. `<data_dir>/packages/package-audit.jsonl` contains only timestamp,
route, release/target, status, and byte count—never headers, queries,
credentials, or terminal data.

## Enroll a worker

### Make one Machine the control

On the Machine you choose as control, add to its `daemon.toml`:

```toml
bind = "127.0.0.1:7420"                 # published only through Tailscale Serve
behind_tailscale_serve = true           # lockout keys on Tailscale-User-Login
allowed_origins = ["https://sentinal-control.TAILNET.ts.net"]

[control]
enabled = true
public_url = "https://sentinal-control.TAILNET.ts.net"
```

Publish the API next to the package role and check it:

```console
control$ tailscale serve --bg --https=443 --set-path /api http://127.0.0.1:7420/api
control$ tailscale serve status
control$ curl --fail https://sentinal-control.TAILNET.ts.net/api/v1/health
```

The `/` mapping serves the installed production PWA and package resources
from port 7422; `/api` takes precedence and goes to the authenticated daemon
API. This is the supported phone/PWA deployment. Vite remains a development
tool and is not used for production or real-device acceptance.

`allowed_origins` lists exact browser origins. An empty list admits no
browser at all (the CLI and worker links still work). `*`, `null`, paths,
and plain-HTTP origins are refused unless `development = true`, which only
the `just` launchers write.

Tailnet ACL (grants) for the intended devices only:

```json
{"src": ["autogroup:owner"], "dst": ["tag:sentinal-control"], "ip": ["443"]},
{"src": ["tag:sentinal-worker"], "dst": ["tag:sentinal-control"], "ip": ["443"]},
{"src": ["tag:sentinal-control"], "dst": ["tag:sentinal-worker"], "ip": ["7420"]}
```

### Connect a worker

1. In the control UI, open Settings → Machines → **Add Machine**. It shows a
   one-time code (10 minutes, one use) and a command without the code.
2. On the already-installed worker:

   ```console
   worker$ sentinal connect https://sentinal-control.TAILNET.ts.net --name "RTX PC"
   Machine ID: 01J9Z3K8V6Q2T7M4X5C0RNB8DA
   Pairing code: [hidden input]
   Connected as RTX PC
   ```

   For automation, pipe the code instead: `sentinal connect URL --code-stdin`.
   A code in argv, a URL, or the environment is always refused.
3. The dialog switches to Connected and the Machine appears in the list.

Manage it afterwards:

```console
worker$ sentinal connection status          # exit 0 connected, 1 not, 3 unreachable, 4 revoked
worker$ sentinal disconnect                 # keeps the Machine ID
worker$ sentinal identity reset             # only for a corrupt or unwanted ID
```

Rename and Revoke live in Settings → Machines and act on the Machine ID;
display names may repeat. Revoking one Machine never affects another, the
owner's browser Session, or the revoked Machine's local Sessions. A revoked
or disconnected worker reconnects with a new code and keeps its ID and name.

The Machine ID is created once, at the daemon's first start, in
`~/.config/terminal-agent/machine-id.json` (mode 0600). Reinstall and update
never replace it. If it is corrupt, the daemon keeps serving local Sessions,
refuses Machine links, and `sentinal identity reset` replaces it explicitly.

**Upgrading an existing install:** `daemon.toml` is preserved, so browsers
are refused until you add `allowed_origins` (for the `just control` proxy in
development: `development = true` and `allowed_origins =
["http://CONTROL_IP:5173"]`).

## Configure real notifications

Notification configuration is Machine-local and independent from package,
owner-login, and Machine-link credentials. On every control or worker Machine
that should deliver notifications, create the default file without putting a
token in argv or shell history:

```console
$ umask 077
$ mkdir -p ~/.config/terminal-agent
$ $EDITOR ~/.config/terminal-agent/notify.toml
$ chmod 600 ~/.config/terminal-agent/notify.toml
```

Use this shape, entering the real values only in the editor:

```toml
server = "https://ntfy.example"
topic = "PRIVATE_TOPIC"
token = "OPTIONAL_PRIVATE_TOKEN"
```

The file and its parent must belong to the current user; the file must be a
regular non-symlink with mode `0600`. Production requires HTTPS. A missing
file reports `unavailable`; an unsafe or invalid file reports `invalid` with a
redacted field-level diagnostic. `sentinald` reloads a valid change without
requiring a restart and keeps the last known-good configuration if a later
edit is invalid.

Settings → Notifications shows each Machine's configuration state, server
origin, topic, whether a token is configured, and bounded delivery history.
It never returns the token. Delivery states distinguish `queued`, `retrying`,
`delivered`, `permanently failed`, `deduplicated`, `rule disabled`, and
`not configured`. Previews are explicitly labelled; ntfy and the phone OS
control final lock-screen layout and colors.

Test the real pipeline with a normal watched Command rather than a fabricated
UI Event:

```console
$ sentinal watch -t "Notification success" -- sh -c 'exit 0'
$ sentinal watch -t "Notification failure" -- sh -c 'exit 7'
$ sentinal watch -t "Waiting check" -- sh -c 'printf "Continue? [y/N] "; read answer'
```

For repeatable production acceptance from a source checkout, the equivalent
dependency-free examples are:

```console
$ bash scripts/run-notification-demo.sh success
$ bash scripts/run-notification-demo.sh failure
$ bash scripts/run-notification-demo.sh waiting
$ bash scripts/run-notification-demo.sh training
```

They create ordinary FULL Sessions and feed the normal Command/watcher/Event
pipeline; they do not call notification APIs or insert Events directly. The
launcher stages the selected program in an owner-only temporary runtime
directory before starting Watch. This keeps a launchd-owned macOS worker out
of privacy-protected source checkouts such as `~/Desktop`, which it cannot read
merely because Terminal.app can. The training example writes one tiny file
under that runtime directory's `artifacts/` folder before announcing its
checkpoint. Checkpoint and progress rules are off by default and must be
enabled for that Session when those categories are under test.

Detach with `Ctrl+J` after starting the waiting test. The stored structured
Event drives delivery; notification text never copies the terminal line or
Command. Retriable timeout/connection/`429`/`5xx` results use four total
attempts with bounded backoff and survive daemon restart in SQLite. Permanent
`4xx` responses stop. Delivery work is bounded to four concurrent requests.

Back up `notify.toml` only through a credential-safe secret backup. To disable
notifications without exposing it, move or delete that exact file and verify
Settings reports unavailable. Never paste it into logs or bug reports.

## Production PWA install, update, and recovery

The worker release includes `web/dist`, and the installer atomically installs
it under `~/.local/share/sentinal/web`. Configure `[packages].web_root` to that
absolute directory and open the `.ts.net` HTTPS origin on the phone. Use the
browser's Add to Home Screen/Install action; login uses a Secure HttpOnly
cookie, not browser storage or a WebSocket query token.

The service worker caches only the public shell. Navigation is network-first,
and an available update shows an explicit Refresh action. Package rollback
restores the previous complete shell. If a phone retained a broken shell,
remove the installed PWA/site data and reopen the HTTPS origin; no Session
output or authentication data is meant to exist in Cache Storage.

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

For the normal persistent multi-Machine control, configure the installed
daemon with `[control]` as above and run:

```console
control$ just control
```

With no `WORKER_IP`, the launcher requires an installed config with the
control role enabled, reads its loopback or Tailscale bind, and starts only
the real-data Vite dashboard. This avoids running a second daemon against the
same Session store and private Unix socket. For compatibility, an explicit
`WORKER_IP` equal to the current Machine also resolves to that installed
control instead of trying its loopback-only daemon through the Tailscale IP.

## Service operations

Linux user service:

```console
$ loginctl show-user "$USER" -p Linger
$ sudo loginctl enable-linger "$USER" # once, when the result was Linger=no
$ systemctl --user status sentinald.service
$ journalctl --user -u sentinald.service --since today
$ systemctl --user restart sentinald.service
```

The systemd user manager must remain active after logout for a worker to be
persistent. On distributions where `loginctl show-user "$USER" -p Linger`
reports `Linger=no`, enable lingering once with the command above. Otherwise
closing the last login can stop `sentinald` and interrupt every live FULL
Session even though the service itself is enabled.

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

For a worker installed from the Tailnet package service, prefer `sentinal
update --check` and `sentinal update`; both reuse the stored verified control
origin. Re-running the original curl command is also an update.

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
4. Detach with `Ctrl+J` (or the legacy `Ctrl-\`, then `d`), or close that
   terminal. Confirm the process and Session remain live in the browser.
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

## Native Web Push setup and recovery

Open the production PWA at the control's Tailnet HTTPS origin. In **Settings →
Notifications**, press **Enable notifications on this device**. That visible
gesture requests permission, registers the browser-managed subscription, and
selects `WEB_PUSH`. On iOS, install Sentinal on the Home Screen first; support
still depends on the installed OS/browser version.

The control creates its VAPID key on the first owner key/subscription action:

```console
$ stat ~/.config/terminal-agent/push-vapid.json   # owner-only 0600
```

Back up that file with the control SQLite database while `sentinald` is
stopped. Losing or rotating it invalidates existing subscriptions, so devices
must subscribe again. Reinstall, upgrade, and normal uninstall preserve both.
Never print the private key, endpoint, browser keys, Machine credentials, or
notification database rows.

`notify.toml` remains intact. Provider selection is exclusive, so `WEB_PUSH`
does not also send the same intent through ntfy. Removing one device affects
only its subscription; logout and owner-password changes retain subscriptions
until explicit removal.

When troubleshooting, distinguish Event/rule decision, worker outbox, control
acceptance, retrying, provider accepted, expired device, and permanent failure.
Provider acceptance does not prove a phone displayed the notification. The
control needs outbound Internet; notification taps also need Tailscale access
to load the private control PWA.

The Notifications page reads secret-free provider state from
`GET /api/v1/push/deliveries`. `CONTROL_ACCEPTED` means a worker intent was
durably accepted by the control; only `DELIVERED` means the selected browser
push provider accepted the encrypted request. Neither state proves that the
phone displayed it. Subscription endpoints and browser keys are never returned
by this resource.

## Current boundaries

- Machine enrollment, isolated heartbeats/revocation, concurrent aggregation,
  worker identity proof, and Machine-scoped terminal/data routing are
  implemented. There is no vendor cloud or required central service.
- The production PWA is installed with the worker bundle and served from the
  authenticated Tailnet control origin; Vite is development-only.
- The worker package is `sentinald`, `sentinal`, shell assets, and the PWA
  shell.
  There is no separate watcher-agent process; Milestone 3's deterministic
  watcher is linked into `sentinald`.
- Tailscale supplies private networking and access policy. Sentinal does not
  configure the Tailnet or bypass host firewalls.
- Browser/CLI disconnects do not end a Session. A daemon restart is different:
  FULL Sessions are interrupted and their PTYs are not reattachable yet.
- Watcher-derived progress, health, errors, structured training summaries,
  and adopted-log tailing are implemented. Real mode shows only data supplied
  by the selected daemon; fixture data requires explicit demo mode.
- Credential-refresh recovery, bounded sensitive storage, login-throttle
  hardening, and the real two-Machine/phone V1 run remain
  release gates in `docs/AUDIT_TRACKER.md`.

## Onboarding boundary

Installation and enrollment stay two deliberate steps:

```text
curl install → UNPAIRED worker
sentinal connect → PAIRED Machine
```

The package carries no pairing code or Machine credential, and nothing in
install, update, or service startup pairs implicitly.
