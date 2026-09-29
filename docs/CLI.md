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
sentinal [--url ws://127.0.0.1:7420/api/v1/ws] [--token-file PATH] <command>
```

The token file defaults to `~/.config/terminal-agent/token`.

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
processes. Tailnet policy and the host firewall must permit ports 5173 and
7420. Check the generated configuration without starting anything with:

```console
$ just network --dry-run
```

Useful overrides are `SENTINAL_TAILSCALE_IP`, `SENTINAL_DAEMON_PORT`,
`SENTINAL_WEB_PORT`, `SENTINAL_DATA_DIR`, and `SENTINAL_CONFIG_DIR`.

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
```

- `ls` lists Session ID, state, cwd, and shell.
- `new` creates a FULL persistent Session and prints its ID.
- `attach` connects the current terminal. Detach with `Ctrl-\` then `d`.
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
Detach with `Ctrl-\` then `d`; EOF, a crashed client, or a closed window
also only detach. The shell and anything it runs keep going; reattach with
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

Registers an existing same-user process read-only as `ADOPTED` / `PROCESS`
over the private local socket.

- `adopt PID [-t TITLE]` adopts one process; `--log PATH` records a readable
  regular log file (tailing lands in 1b.D).
- `adopt --pick` lists your processes (newest first, excluding itself, its
  shell, and `sentinald`) and adopts only after you type a list number or
  PID; Enter or `q` adopts nothing.
- Exit status 0 on adoption or cancel; 1 when the daemon is unreachable or
  refuses (dead PID, another user's process, `sentinald`, already adopted,
  bad log). Nothing is adopted on any error.
- Sentinal never signals, traces, or writes to an adopted process; its exit
  code is always shown as unknown.

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
| `SENTINAL_TAILSCALE_IP` | development | overrides Tailscale IPv4 discovery for `just network`; still validated as `100.64.0.0/10` |
| `SENTINAL_DAEMON_PORT` | development | daemon port used by `just network` (default `7420`) |
| `SENTINAL_WEB_PORT` | development | Vite port used by `just network` (default `5173`) |
| `SENTINAL_DATA_DIR` | development | persistent data path used by `just network` |
| `SENTINAL_CONFIG_DIR` | development | token/auth path used by `just network` |
| `TA_CONFIG` | current internal | overrides `sentinald` configuration path |
| `TA_AUTH_FILE` | current test/internal | overrides owner-auth storage |
| `TA_FAKE_PTY` | current test-only | selects the fake PTY backend |

Public `SENTINAL_*` replacements for internal `TA_*` overrides are part of
the naming migration and require compatibility tests before removal of the
old names.
