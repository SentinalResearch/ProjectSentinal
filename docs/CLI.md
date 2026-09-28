# CLI reference

The current binary is `ta`. Milestone 1b changes the installed product name
to `sentinel`; until that migration lands, commands marked **planned** are not
available.

## Global options (current)

```console
ta [--url ws://127.0.0.1:7420/api/v1/ws] [--token-file PATH] <command>
```

The token file defaults to `~/.config/terminal-agent/token`.

## Current commands

```console
ta ls
ta new [--cwd DIR]
ta attach <SESSION_ID>
ta kill <SESSION_ID>
```

- `ls` lists Session ID, state, cwd, and shell.
- `new` creates a FULL persistent Session and prints its ID.
- `attach` connects the current terminal. Detach with `Ctrl-\` then `d`.
- `kill` terminates a daemon-owned Session and marks it interrupted.

## Milestone 1b target commands

```console
sentinel ls
sentinel new [--cwd DIR]
sentinel attach <SESSION_ID>
sentinel kill <SESSION_ID>

sentinel watch
sentinel watch [-t TITLE] -- COMMAND [ARG...]

sentinel hook zsh|bash|fish
sentinel hook install [zsh|bash|fish]
sentinel hook uninstall [zsh|bash|fish]

sentinel auto on|off|status

sentinel adopt PID [--log PATH] [-t TITLE]
sentinel adopt --pick
```

### `watch` (planned)

Creates a `WATCH` / `FULL` Session in the current directory and attaches in
the same window. Arguments after `--` are preserved as structured arguments.
The shell remains available after the initial Command exits. A nested Watch
returns the existing Sentinel context without creating another Session.

Exit behavior:

- `0`: detached normally or an existing watched context was reused;
- non-zero: daemon unavailable, request rejected, or local attach failed;
- the supplied Command is never run unwatched after a creation failure.

### `hook` (planned external mode)

`hook zsh|bash|fish` prints the integration without editing files. `install`
adds one marker-delimited block and is idempotent. `uninstall` removes only
the Sentinel block and restores all surrounding bytes exactly.

### `auto` (planned)

Enables, disables, or reports opt-in shell auto-wrap. `on` must describe the
config file it changes. `SENTINEL_DISABLE=1` always bypasses it.

### `adopt` (planned)

Registers a same-user process read-only. `--log` selects an explicit log and
upgrades capture to PROCESS_LOG. `--pick` offers same-user candidates without
changing them. An adopted process's exit code is always unknown.

## Environment

| Variable | Status | Meaning |
|---|---|---|
| `SENTINEL_SESSION_ID` | planned | identifies a FULL Session and prevents nesting/double reporting |
| `SENTINEL_DISABLE=1` | planned | disables external hook reporting and auto-wrap |
| `TA_CONFIG` | current internal | overrides daemon configuration path |
| `TA_AUTH_FILE` | current test/internal | overrides owner-auth storage |
| `TA_FAKE_PTY` | current test-only | selects the fake PTY backend |

Public `SENTINEL_*` replacements for internal `TA_*` overrides are part of
the naming migration and require compatibility tests before removal of the
old names.
