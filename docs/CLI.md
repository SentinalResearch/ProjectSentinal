# CLI reference

The installed client is `sentinal` and the daemon is `sentinald`. `ta` and
`tad` remain as compatibility aliases of the same programs through the
Milestone 1b migration (same arguments, same behavior); existing
`tad.service` / `dev.terminal-agent.tad.plist` units keep working. New units
are `packaging/systemd/sentinald.service` and
`packaging/launchd/dev.sentinal.sentinald.plist`; enable only one daemon unit.
Commands marked **planned** are not available yet.

## Global options

```console
sentinal [--url ws://HOST:PORT/api/v1/ws] [--token-file PATH] <command>
```

The token file defaults to `~/.config/terminal-agent/token`. Without
`--url` or `SENTINAL_URL`, the CLI reads `bind` from
`~/.config/terminal-agent/daemon.toml`; if that config does not exist it uses
the loopback development default `ws://127.0.0.1:7420/api/v1/ws`. This makes
an installed CLI reach its Tailscale-bound worker without duplicating the
address in shell configuration.

## Tailscale development server

From the repository root, one command starts both `sentinald` and the Vite
frontend on this machine's Tailscale IPv4 address. This command always uses
the real daemon API; fixture data cannot leak into a handoff test:

```console
$ just network
Sentinal Tailscale development server
Web UI:    http://100.x.y.z:5173
Daemon API: http://100.x.y.z:7420
```

Open the displayed Web UI URL on another device connected to the same
Tailnet. On the first run, read the bootstrap token in a second SSH terminal
with the exact `cat .../token` command printed by the launcher, paste it into
the setup page, and choose the single owner's password. Later visits use
that password; the launcher does not create or print a default password.

The launcher binds to one `100.64.0.0/10` Tailscale address. It refuses
`0.0.0.0`, ordinary LAN addresses, and non-Tailscale overrides. It is a
development server: keep the terminal open, and press Ctrl-C to stop both
processes. It also refuses a stale assigned address while Tailscale is stopped
or still starting; turn Tailscale on and wait until it reports connected.
Tailnet policy and the host firewall must permit ports 5173 and 7420. Check
the generated configuration without starting anything with:

```console
$ just network --dry-run
```

Useful overrides are `SENTINAL_TAILSCALE_IP`, `SENTINAL_DAEMON_PORT`,
`SENTINAL_WEB_PORT`, `SENTINAL_DATA_DIR`, and `SENTINAL_CONFIG_DIR`.

### Persistent control dashboard and direct-worker compatibility

When this Machine's installed daemon has `[control]` enabled, start only its
real-data dashboard. The launcher reads the daemon bind from the preserved
config and does not start a second daemon:

```console
control$ just control
```

For the older Milestone 1c direct-worker shape, select one worker explicitly:

```console
worker$ just agent
control$ just control WORKER_IP=100.x.y.z
```

Both commands bind only to the exact local Tailscale IPv4. `just control`
always selects the real API adapter and proxies HTTP and WebSocket traffic to
the installed control or explicitly named worker; it never starts a daemon or
falls back to mock data. The source-tree `just agent` prints the explicit
`SENTINAL_URL` needed by a CLI in another terminal. An installed CLI instead
discovers the worker bind from its persistent daemon config.
If `WORKER_IP` names the current Machine and its installed daemon has the
control role, the launcher uses that daemon's configured bind automatically;
an old same-Machine command therefore cannot bypass a loopback-only control.

Install or upgrade the worker with `just install-worker`; remove programs and
the user service with `just uninstall-worker`. Config and data are preserved
unless exact-path data deletion is separately requested. Full prerequisites,
service commands, paths, backup, and acceptance steps are in
`docs/SETUP.md`.

### `setup`

`sentinal setup` asks what this Machine is, previews every change, and
applies it only after `y` (or `--yes`). Re-running it finishes what is missing
and otherwise prints "Nothing to do".

- **Control:** rewrites `daemon.toml` to bind `127.0.0.1:7420` with
  `[control]` and `[packages]` (the previous file is kept as
  `daemon.toml.before-setup`; comments and other keys are kept). It then
  enables linger on Linux (`sudo loginctl enable-linger`, shown first), restarts
  the service, checks health, adds the Tailscale Serve routes `/` → `:7422` and
  `/api` → `:7420/api`, checks health through HTTPS, and publishes the
  installed signed release for workers. It never binds `0.0.0.0`, never turns on
  Funnel, and never replaces a Serve route it did not create. If the restarted
  daemon is unhealthy, the previous config is restored.
- **Worker:** option 2 lists the controls found on the Tailnet (name,
  origin, release, and a short control ID) and never picks one by itself;
  option 3, or `--control-url`, takes a typed URL. Either way it continues with
  the one-time code (`connect`) when the control predates approval pairing.
  A discovered control that supports approval gets a request instead: setup
  prints four verification words and the control's name and short ID, and the
  owner approves the request whose words match in the dashboard. The daemon
  holds the request secret; setup only asks it to claim every 2 s. Ctrl-C
  withdraws the request. Exit 4 means rejected or expired (10 minutes).
  Discovery reads only `tailscale status --json`: online peers with a `.ts.net`
  name and a Tailscale address, at most 64 peers and 8 probes at a time, about
  6 s in all. Each probe is `https://<peer>/api/v1/discovery`, pinned to that
  peer's Tailscale address, with no redirects and a 4 KiB body cap. An answer
  counts only if it names exactly the probed origin. A 404 is listed as an
  older Sentinal.

- **Worker → another control:** on a connected worker, option 2 lists the
  *other* controls and asks the chosen one for approval
  (`keep_current_link`). The worker stays connected until that control's
  owner approves it, then switches and tells the old control it left.
- **New Tailscale address:** on a worker bound to a Tailscale address that
  is no longer this Machine's (it joined another Tailnet), setup previews
  and applies the new `bind` first, keeping `daemon.toml.before-rebind`.
- **Control → worker:** on a control, `sentinal setup --role worker` (or
  option 2) lists the *other* controls on the Tailnet and asks which to join.
  After a preview and confirmation, it:
  1. tells each active worker over its link to ask that control for approval.
     The workers keep their current link until you approve them in the new
     control's dashboard.
  2. rewrites `daemon.toml` as a worker config bound to this Machine's
     Tailscale address. The control config is kept as
     `daemon.toml.control-backup`.
  3. restarts and health-checks the daemon.
  4. removes this Machine's Serve routes.
  5. asks the chosen control to approve this Machine, showing the
     verification words.

  A failed restart restores the control config. Workers that cannot be
  reached are listed; run `sentinal setup` on them later. Choosing *None yet*
  makes an unconnected worker, after warning which workers lose their
  control.

Exit codes: 0 done or nothing to do · 1 a step failed (files restored) ·
2 cancelled or usage · 3–5 as `connect`, with 5 also meaning "already set up
differently" (the file and reason are named) · 6 Tailscale missing or signed
out. Without a terminal, pass `--role control --yes`, or
`--role worker --control-url URL --code-stdin`.

### `backup`

`sentinal backup create` stops `sentinald`, writes an owner-only (0600)
`tar.gz` of the config and data directories (default
`~/sentinal-backups/sentinal-backup-<time>.tar.gz`), and starts it again. The
archive holds credentials and terminal output; keep it private.
`sentinal backup restore FILE` needs a stopped service and empty directories,
and refuses any entry outside the two Sentinal directories. See
`docs/PILOT.md`.

### Private install and `connect` flow

An owner-controlled Tailnet control Machine can serve a
verified `install.sh` and versioned worker artifacts. Installation ends
`UNPAIRED`; it never consumes a pairing code or registers the Machine.

Enrollment is a separate, explicit step (Milestone 4.0):

```console
sentinal connect CONTROL_URL [--name NAME]
sentinal connect CONTROL_URL [--name NAME] --code-stdin
sentinal connection status [--json]
sentinal disconnect
sentinal identity reset [--yes]
```

`connect` prints the Machine ID, then reads the one-time code from
`/dev/tty` with echo, line editing, and signals off (Ctrl-C cancels and
restores the terminal); a wrong code may be retyped up to 3 times. With
`--code-stdin` it reads one line of at most 64 bytes. Before argument parsing,
every `--code`, `--code=`, `--pairing-code`, `-c`, extra positional, and URL
with a query, fragment, or credentials is refused without echoing it, so a
code never enters argv, a URL, the environment, process listings, or shell
history. The local daemon performs the HTTPS exchange.

`CONTROL_URL` must be `https://<machine>.<tailnet>.ts.net`; plain
`http://` to loopback or a Tailscale IPv4 address is accepted only by a
daemon with `development = true`.

| Command | 0 | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|---|
| `connect` | connected | internal | usage, code in argv, no terminal | control unreachable or not a control | code rejected / pairing locked | already connected | `sentinald` unavailable or identity corrupt |
| `connection status` | connected | not connected | usage | control unreachable | revoked | — | `sentinald` unavailable |
| `disconnect` | removed (warns if the control was not told) | not connected | usage | — | — | — | `sentinald` unavailable |

An already-connected Machine refuses `connect` (exit 5); run `sentinal
disconnect` first. The Machine ID survives rename, disconnect, reconnect,
reinstall, and update; only `identity reset` (after typing `RESET`, or
`--yes`) replaces it.

Package installation and maintenance commands available now are:

```console
curl -fsSL https://sentinal-control.TAILNET.ts.net/install.sh | sh
sentinal doctor [--prefix DIR] [--config-dir DIR]
sentinal update --check [--prefix DIR]
sentinal update [--yes] [--prefix DIR]
sentinal update --rollback VERSION [--yes] [--prefix DIR]
```

`update` restarts sentinald, which ends this Machine's live terminals (until
Reliability R1). When any are running it says so ("This ends 2 live terminals
on pop-os.", adding "including this one" inside a Session) and asks; without a
terminal to ask on it stops unless `--yes` is given.

`doctor` reads local installation/configuration state and invokes the native
service manager and Tailscale status commands. It makes no network request and
changes nothing. Besides Binaries, Config, Tailscale, and Service it prints:

- `Version`: the installed release and target;
- `Role`: `WORKER`, or `CONTROL (<public URL>)`;
- `Pairing`: `CONNECTED to <control> as <name>`, `NOT CONNECTED`, or `REVOKED`,
  read from the link file. It never prints a credential. A control prints
  "not needed on the control".
- `Updates`: the worker's control origin and channel, or `sentinal release update` on a control.
- `--pilot` (a control only) adds read-only `Pilot …` lines: listeners on
  7420/7422 loopback-only, exactly the two Serve routes with Funnel off,
  HTTPS health/dashboard/`install.sh`, the stable release, and a backup no
  older than 7 days. Any `ERROR` makes `doctor` exit non-zero.
- `Storage`: saved Session logs (size, count) against `[storage]`
  `session_log_max_bytes` and the age limit, plus audit-file and database size.
  It reads file sizes only.

Tailscale is found on `PATH`, then inside the macOS app bundle
(`/Applications/Tailscale.app/Contents/MacOS/Tailscale`).

`sentinal --version` prints the release and short commit stamped by the
release build, for example `sentinal 0.1.16 (ac92ae5)`. Source builds print
`0.0.1 (development build)`.

Install and update print four steps: Downloading (with a progress bar in a
terminal), Verifying, Installing, and Checking. Checking reports the real link
state after the restart, and prints a `PATH` hint when `<prefix>/bin` is not on `PATH`.

`update` reads the credential-free HTTPS `.ts.net` origin from
`<prefix>/share/sentinal/release.json`, downloads the same bounded bootstrap
used by fresh installation, and reuses its target, length, and SHA-256 checks.
`--check` downloads no artifact. A downgrade requires `--rollback` with the
exact version after the control owner has moved the stable pointer.

### Explicit fixture demo

Real daemon data is the default for ordinary Vite builds and development.
To show the complete future-facing design without a daemon, opt into the
clearly labelled local fixture demo:

```console
$ just demo
# opens http://127.0.0.1:5173 with visible MOCK labels
```

The underlying setting is `VITE_DATA=mock`; any missing, misspelled, or
`api` value selects the real API. Demo mode is never selected by
`just network`.

For facilitated interface research, use the same isolated fixture boundary
with repeatable controls:

```console
$ just user-test
# Settings → Simulation
```

The Simulation tab provides named scenarios, pause/normal/fast playback, a
manual tick, and baseline reset. It does not exist in real API mode. See
`docs/USER_TESTING.md` for the task script and what may be claimed from a
simulation.

## Commands

```console
sentinal ls
sentinal top [--once]
sentinal status <SESSION_ID>
sentinal new [--cwd DIR]
sentinal attach <SESSION_ID>
sentinal kill <SESSION_ID>

sentinal watch
sentinal watch -- COMMAND [ARG...]
sentinal watch -t TITLE -- COMMAND [ARG...]

sentinal hook zsh|bash|fish
sentinal hook install [zsh|bash|fish] [--rc PATH]
sentinal hook uninstall [zsh|bash|fish] [--rc PATH]
sentinal auto on|off|status [zsh|bash|fish] [--rc PATH]
sentinal adopt PID [--log PATH] [-t TITLE]
sentinal adopt --pick
sentinal setup [--role control|worker] [--name NAME] [--control-url URL] [--yes] [--code-stdin]
sentinal connect CONTROL_URL [--name NAME] [--code-stdin]
sentinal connection status [--json]
sentinal disconnect
sentinal identity reset [--yes]
sentinal doctor [--prefix DIR] [--config-dir DIR] [--pilot]
sentinal backup create [--output FILE] [--config-dir DIR]
sentinal backup restore FILE [--config-dir DIR]
sentinal update [--check | --rollback VERSION] [--yes] [--prefix DIR]
sentinal release check [--channel stable|beta] [--json]
sentinal release update [--version V] [--yes] [--skip-provenance] [--health-timeout S]
sentinal release rollback VERSION [--yes]
sentinal release policy [notify|auto|off] [--channel stable|beta]
```

`sentinal release` runs on the control only (a worker is refused and pointed
to `sentinal update`). It trusts only releases signed by the keys compiled into
the binary, or by `--allowed-signers FILE` when given explicitly.

Notifications have no synthetic `test` command. Configure `notify.toml` as
described in `docs/SETUP.md`, then run an ordinary watched or hooked Command;
its stored structured Event is the production path. Delivery/configuration
state and privacy-safe previews are visible under Settings → Notifications.
No CLI command prints the ntfy token.

- `ls` lists Session ID, state, cwd, and shell.
- `top` is a text view of this Machine, refreshed every 2 s until Ctrl-C:
  - host, OS, Sentinal version and uptime;
  - CPU, RAM, GPU and VRAM bars;
  - a table of live Sessions with state, capture, progress, loss and trend,
    CPU, memory and health.

  `--once` (or output that is not a terminal) prints one snapshot. GPU numbers
  come from `nvidia-smi` when it is installed.
- `status` shows one Session:
  - progress bar;
  - training loss (with its first value, trend, best validation loss and
    learning rate);
  - health and its latest evidence;
  - busiest processes (names only, never command lines);
  - last eight Events.
- `new` creates a FULL persistent Session and prints its ID.
- `attach` connects the current terminal. Detach with `Ctrl+J`. The legacy
  `Ctrl-\`, then `d` sequence remains available for compatibility.
  Detaching a live Session never ends it. Finished Sessions are records; use
  the web read-only view or the authenticated replay resource below.
- `kill` terminates a live daemon-owned Session and marks it interrupted. A
  second kill, or any input/resize/signal after the Session ends, is rejected
  before the closed PTY and input audit log are touched.

### Finished Session output

The web UI opens `COMPLETED`, `FAILED`, and `INTERRUPTED` FULL Sessions in a
read-only xterm view and pages their saved ANSI bytes from the daemon. It
shows state and known exit code, removes input/resize/mobile-key/kill actions,
and offers **Continue in new Session**. Continue creates a distinct UI/FULL
Session from the old shell, cwd, known dimensions, and a derived title. It
does not reuse the old ID or run the old Command.

The same authenticated resource can export ANSI bytes without revealing a
server filesystem path. Paste either a browser-session token or the Machine
token into `TOKEN`, then request pages until
`X-Sentinal-Output-Complete: true`:

```sh
TOKEN='paste-token-here'
curl --fail-with-body \
  -H "Authorization: Bearer $TOKEN" \
  -D session-12.headers \
  -o session-12.part-0.ansi \
  'http://127.0.0.1:7420/api/v1/sessions/12/output?offset=0&limit=1048576'
```

If the response says it is incomplete, copy
`X-Sentinal-Output-Next-Offset` into the next request's `offset` and append
that response to the export. One page is at most 1 MiB and one request scans
at most 64 MiB of decompressed output. ANSI/control bytes are preserved and
must be opened in a terminal emulator or treated as binary data, never HTML
or shell text. `X-Sentinal-Output-Warning: TRUNCATED_ZSTD` means verified
preceding output was returned but the final compressed frame was incomplete.

### `watch`

Creates a `WATCH` / `FULL` Session and attaches the current terminal to it
immediately. The Session uses the caller's current directory, current
terminal size (80x24 when unknown or reported as 0x0), and `$SHELL`.
Detach with `Ctrl+J`; the legacy `Ctrl-\`, then `d` sequence also works. EOF,
a crashed client, or a closed window also only detach. The shell and anything
it runs keep going; reattach with
`sentinal attach <ID>` or the web UI.

```console
$ sentinal watch -t "Qwen Training" -- python train.py --config moe.yaml
...
sentinal: detached from Session 12; it keeps running. Reattach with `sentinal attach 12`
```

With `-- COMMAND [ARG...]`:

- the arguments are sent as a structured argv, exactly as the calling shell
  passed them to `sentinal` (no re-splitting or re-expansion);
- the daemon types the Command once, at the Session shell's first prompt,
  rendered by the protocol quoting helper (`protocol::render_command_line`):
  every argument is one quoted word and the executable is always quoted, so
  aliases, keywords, and `NAME=value` cannot change what runs; control and
  (for bash/zsh) non-ASCII bytes are escapes, never raw keystrokes;
- the shell stays open after the Command finishes;
- a Command requires `$SHELL` to be zsh, bash, or fish (the shells with
  Sentinal prompt hooks) and must render to at most 1,000 bytes. Anything
  longer belongs in a script: `sentinal watch -- ./train.sh`;
- `sentinal watch --` with nothing after it is a bare Watch; `-- ""` is
  rejected because the executable is empty.

Inside any daemon-owned FULL Session, `SENTINAL_SESSION_ID` is set before the
user's shell configuration loads. Watch refuses to nest when it is set.

Exit status:

| Status | When |
|---:|---|
| `0` | detached (explicitly, EOF, or closed window), or bare `watch` inside an already-watched terminal (nothing created) |
| `1` | `watch -- COMMAND` inside an already-watched terminal (nothing created, Command not run) |
| `1` | not an interactive terminal, invalid arguments, daemon unavailable, or creation rejected — the Command never runs outside Sentinal |
| `1` | the Session was created but attaching failed — the Command runs inside that Session; the error names the ID to reattach |

### `hook`

Reports Commands from ordinary terminals as `SHELL_HOOK` / `COMMANDS`
Sessions (no output, input, or attach). Full behavior, rc-file selection,
limitations, and measurements are in `docs/SHELL_HOOKS.md`.

- `hook zsh|bash|fish` prints the integration to stdout and edits nothing.
- `install [SHELL] [--rc PATH]` adds one marker-delimited block to the rc
  file for `SHELL` (default: `$SHELL`). A second install changes nothing. An
  owner-only backup is written first, and the file is replaced atomically
  with its permissions preserved.
- `uninstall [SHELL] [--rc PATH]` removes exactly that block and restores the
  rest of the file byte for byte.
- Malformed markers, CRLF files, an ambiguous rc file, or world-writable
  paths produce a clear error and no change. Exit status is 0 on success
  (including "nothing changed") and 1 otherwise.

Reporting itself runs through a hidden `hook report` subcommand that the
hook starts in the background; it is always silent.

### `auto`

Opt-in auto-wrap of new interactive terminals (details and skip table in
`docs/CAPTURE_MODES.md`, Mode 3).

- `auto on [SHELL] [--rc PATH]` adds a block at the top of the rc file and
  prints which file changed and its backup; idempotent.
- `auto off` removes exactly that block.
- `auto status` prints the file and line range of the block (or that it is
  off) and whether a new shell in this terminal would be wrapped, or why not:

```console
$ sentinal auto status
Auto-wrap: on for zsh — block at lines 1–6 of ~/.zshrc
This terminal: inside Sentinal Session 12
```

### `adopt`

Registers an existing same-user process read-only as `ADOPTED` / `PROCESS`, or `PROCESS_LOG` once its log is tailed,
over the private local socket.

- `adopt PID [-t TITLE]` adopts one process; `--log PATH` read-only tails a
  same-user regular log through deterministic watchers and prints
  `[ADOPTED · PROCESS_LOG]`. A log that is recorded but not read is reported
  as such, with Metrics only.
- `adopt --pick` lists your processes (newest first, excluding itself, its
  shell, and `sentinald`) and adopts only after you type a list number or
  PID; Enter or `q` adopts nothing.
- Exit status 0 on adoption or cancel; 1 when the daemon is unreachable or
  refuses (dead PID, another user's process, `sentinald`, already adopted,
  bad log). Nothing is adopted on any error.
- Sentinal never signals, traces, or writes to an adopted process; its exit
  code is always shown as unknown.

## Native notification operations

Native Web Push is managed from the authenticated production PWA under
**Settings → Notifications**. Permission is never requested at startup or by a
CLI command. The page subscribes the current browser, selects Web Push, lists
secret-free device labels/state/last-success time, and removes one device.

There is intentionally no command that places a VAPID private key, Push
endpoint, browser authentication key, or provider credential in argv or shell
history. A future native-app provider uses separate APNs/FCM tokens; browser
Web Push subscriptions cannot be reused as native device tokens.

## Environment

| Variable | Status | Meaning |
|---|---|---|
| `SENTINAL_SESSION_ID` | current | set in every daemon-owned FULL Session; prevents nested Watch (and later double reporting) |
| `SENTINAL_DISABLE=1` | current | disables external hook reporting and auto-wrap |
| `SENTINAL_SOCKET` | current | local socket path for hook reporting when `sentinald` does not use the default |
| `SENTINAL_TITLE` | current | optional title for a shell's SHELL_HOOK Session |
| `SENTINAL_URL` | current | daemon WebSocket URL (same as `--url`) |
| `SENTINAL_TOKEN_FILE` | current | bearer token file (same as `--token-file`) |
| `SENTINAL_AUTO_GUARD` | current | set in every AUTO_WRAP shell; any value stops auto-wrap |
| `SENTINAL_AUTO_SKIP` | current | comma-separated variable names; auto-wrap skips when any is set |
| `SENTINAL_TAILSCALE_CLI` | current | explicit Tailscale CLI path for `install.sh` and `sentinal doctor` |
| `SENTINAL_TAILSCALE_IP` | development | overrides local IPv4 discovery for `just network`, `agent`, or `control`; still validated as `100.64.0.0/10` |
| `SENTINAL_DAEMON_PORT` | development | worker daemon port used by launch roles (default `7420`) |
| `SENTINAL_WEB_PORT` | development | Vite port used by web launch roles (default `5173`) |
| `SENTINAL_DATA_DIR` | development | persistent data path used by daemon launch roles |
| `SENTINAL_CONFIG_DIR` | development | token/auth path used by daemon launch roles |
| `TA_CONFIG` | current internal | overrides `sentinald` configuration path |
| `TA_AUTH_FILE` | current test/internal | overrides owner-auth storage |
| `TA_FAKE_PTY` | current test-only | selects the fake PTY backend |

Public `SENTINAL_*` replacements for internal `TA_*` overrides are part of
the naming migration and require compatibility tests before removal of the
old names.
