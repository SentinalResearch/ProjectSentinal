# CLI reference

The installed client is `sentinel` and the daemon is `sentineld`. `ta` and
`tad` remain as compatibility aliases of the same programs through the
Milestone 1b migration (same arguments, same behavior); existing
`tad.service` / `dev.terminal-agent.tad.plist` units keep working. New units
are `packaging/systemd/sentineld.service` and
`packaging/launchd/dev.sentinel.sentineld.plist`; enable only one daemon unit.
Commands marked **planned** are not available yet.

## Global options

```console
sentinel [--url ws://127.0.0.1:7420/api/v1/ws] [--token-file PATH] <command>
```

The token file defaults to `~/.config/terminal-agent/token`.

## Commands

```console
sentinel ls
sentinel new [--cwd DIR]
sentinel attach <SESSION_ID>
sentinel kill <SESSION_ID>

sentinel watch
sentinel watch -- COMMAND [ARG...]
sentinel watch -t TITLE -- COMMAND [ARG...]

sentinel hook zsh|bash|fish                 # planned
sentinel hook install [zsh|bash|fish]       # planned
sentinel hook uninstall [zsh|bash|fish]     # planned
sentinel auto on|off|status                 # planned
sentinel adopt PID [--log PATH] [-t TITLE]  # planned
sentinel adopt --pick                       # planned
```

- `ls` lists Session ID, state, cwd, and shell.
- `new` creates a FULL persistent Session and prints its ID.
- `attach` connects the current terminal. Detach with `Ctrl-\` then `d`.
- `kill` terminates a daemon-owned Session and marks it interrupted.

### `watch`

Creates a `WATCH` / `FULL` Session and attaches the current terminal to it
immediately. The Session uses the caller's current directory, current
terminal size (80x24 when unknown or reported as 0x0), and `$SHELL`.
Detach with `Ctrl-\` then `d`; EOF, a crashed client, or a closed window
also only detach. The shell and anything it runs keep going; reattach with
`sentinel attach <ID>` or the web UI.

```console
$ sentinel watch -t "Qwen Training" -- python train.py --config moe.yaml
...
sentinel: detached from Session 12; it keeps running. Reattach with `sentinel attach 12`
```

With `-- COMMAND [ARG...]`:

- the arguments are sent as a structured argv, exactly as the calling shell
  passed them to `sentinel` (no re-splitting or re-expansion);
- the daemon types the Command once, at the Session shell's first prompt,
  rendered by the protocol quoting helper (`protocol::render_command_line`):
  every argument is one quoted word and the executable is always quoted, so
  aliases, keywords, and `NAME=value` cannot change what runs; control and
  (for bash/zsh) non-ASCII bytes are escapes, never raw keystrokes;
- the shell stays open after the Command finishes;
- a Command requires `$SHELL` to be zsh, bash, or fish (the shells with
  Sentinel prompt hooks) and must render to at most 1,000 bytes. Anything
  longer belongs in a script: `sentinel watch -- ./train.sh`;
- `sentinel watch --` with nothing after it is a bare Watch; `-- ""` is
  rejected because the executable is empty.

Inside any daemon-owned FULL Session, `SENTINEL_SESSION_ID` is set before the
user's shell configuration loads. Watch refuses to nest when it is set.

Exit status:

| Status | When |
|---:|---|
| `0` | detached (explicitly, EOF, or closed window), or bare `watch` inside an already-watched terminal (nothing created) |
| `1` | `watch -- COMMAND` inside an already-watched terminal (nothing created, Command not run) |
| `1` | not an interactive terminal, invalid arguments, daemon unavailable, or creation rejected — the Command never runs outside Sentinel |
| `1` | the Session was created but attaching failed — the Command runs inside that Session; the error names the ID to reattach |

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
| `SENTINEL_SESSION_ID` | current | set in every daemon-owned FULL Session; prevents nested Watch (and later double reporting) |
| `SENTINEL_DISABLE=1` | planned | disables external hook reporting and auto-wrap |
| `TA_CONFIG` | current internal | overrides `sentineld` configuration path |
| `TA_AUTH_FILE` | current test/internal | overrides owner-auth storage |
| `TA_FAKE_PTY` | current test-only | selects the fake PTY backend |

Public `SENTINEL_*` replacements for internal `TA_*` overrides are part of
the naming migration and require compatibility tests before removal of the
old names.
