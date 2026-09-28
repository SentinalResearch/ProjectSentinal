# Shell hooks

Sentinal has two shell-integration paths. They share Command semantics but
serve different capture modes and must never report the same Command twice.

## Daemon-owned PTY path (current)

When the daemon creates a FULL Session it installs an isolated wrapper for
the selected shell and then loads the user's normal configuration. The
wrapper emits terminal sequences into the PTY stream:

- OSC 133 `A`: prompt start
- OSC 133 `B`: Command start
- OSC 133 `C`: output start
- OSC 133 `D;<exit>`: Command end
- OSC 7 `file://host/path`: cwd
- OSC 7420 `cmd=<base64>`: exact Command text

The streaming daemon parser accepts sequences split at any byte boundary and
passes every byte through unchanged to the screen and clients. Unknown OSC
codes pass through. An unterminated sequence is bounded so it cannot consume
output forever.

Current sources are `shell/ta.zsh`, `shell/ta.bash`, and `shell/ta.fish`.
They are installed into the daemon data directory for Session launch; they do
not edit the owner's rc files.

## Ordinary-shell socket path (Milestone 1b.B)

```console
$ sentinal hook install          # uses $SHELL; or: sentinal hook install zsh|bash|fish [--rc PATH]
$ sentinal hook uninstall
$ sentinal hook zsh              # print the integration; edits nothing
```

`install` adds one marker-delimited block to the shell's rc file. The block
only evaluates `sentinal hook <shell>` when the `sentinal` binary still
exists, so removing Sentinal never breaks a shell:

```sh
# >>> sentinal hook >>>
# Managed by `sentinal hook install`; remove with `sentinal hook uninstall`.
if [ -x '/usr/local/bin/sentinal' ]; then eval "$('/usr/local/bin/sentinal' hook zsh 2>/dev/null)"; fi
# <<< sentinal hook <<<
```

An interactive shell then registers itself as a `SHELL_HOOK` / `COMMANDS`
Session rooted at its PID and reports each Command's exact text, cwd, start,
finish, and exit status through the private local socket. Hook sources are
`shell/external/sentinal.{zsh,bash,fish}`.

### Runtime behavior

- Each event starts one background `sentinal hook report` process and the
  prompt never waits for it. The reporter registers the shell (idempotent on
  the daemon) and sends one `SHELL_COMMAND_STARTED` or
  `SHELL_COMMAND_FINISHED` within a 50 ms deadline. Socket missing, daemon
  down, timeout, and malformed replies are all silent; the reporter always
  exits 0 with no output, and the hook discards its output anyway.
- Command text is passed to the reporter in the `SENTINAL_HOOK_TEXT`
  environment variable, not argv, so other users cannot see it in `ps`.
  Output, stdin, keystrokes, and the rest of the environment are never sent.
- The Command's exit status is captured first and restored after hook
  bookkeeping, so `$?` / `$status` and prompts that show it are unchanged.
- zsh uses `preexec`/`precmd` (its precmd runs first). bash uses a DEBUG trap
  armed by the last `PROMPT_COMMAND` entry, reads the full line from
  `history 1`, and registers with bash-preexec's arrays instead when
  bash-preexec is loaded. fish uses `fish_preexec` / `fish_postexec`.
- Each shell numbers its Commands. The daemon applies a finish that arrives
  before its start, ignores repeats, clamps finish >= start, and does not let a
  lost finish block the next Command.
- `SENTINAL_SOCKET` overrides the socket path; set it in the shell's
  environment when `sentinald` uses a non-default `data_dir` or
  `local_socket`. `SENTINAL_TITLE` sets an optional Session title.

### Lifecycle

- The shell PID is the Session's process-tree root, so the existing Metrics
  sampler covers it and everything it runs.
- `sentinald` checks every 2 s that the PID still exists with the same owner
  and OS start time. When the shell exits (or its PID is reused), the Session
  becomes `COMPLETED` with no exit code ("Ended · exit code unknown") and a
  `PROCESS_EXITED` event is recorded.
- COMMANDS Sessions cannot be attached, typed into, resized, or killed; the
  daemon's capture check refuses those operations.
- On daemon restart, a SHELL_HOOK Session is resumed only if its PID still has
  the recorded owner and start time; otherwise it is ended as above. A Command
  that was running across the restart stays without a finish time.

### Known limitations

- bash records the full line from history. When history does not record it
  (`HISTCONTROL=ignorespace`/`ignoredups`, `set +o history`), the text falls
  back to the first simple command (`$BASH_COMMAND`). A Command whose text
  exactly equals a `PROMPT_COMMAND` entry is not reported.
- If another tool already owns bash's DEBUG trap and bash-preexec is not
  loaded, the bash hook stays inactive instead of replacing that trap.
- zsh and bash 5+ stamp times in the shell (`$EPOCHREALTIME`); bash 3.2/4 and
  fish are stamped by the reporter, a few milliseconds after the event.
- OS process start times have one-second precision, so a PID reused within the
  same second as the original shell's start would not be detected.

### Suppression

If `SENTINAL_SESSION_ID` is set, the socket reporter is disabled and the
existing OSC path remains active. This is the no-double-counting rule. It is
checked when the hook loads, again on every event, and by the reporter.

If `SENTINAL_DISABLE=1` is set, all external reporting and auto-wrap are
disabled. The daemon-owned OSC wrapper may still track the FULL Session that
the user explicitly entered.

## Install and uninstall contract

- The shell comes from the argument or `$SHELL`; anything else is an error.
- rc files: zsh `${ZDOTDIR:-~}/.zshrc`; fish
  `${XDG_CONFIG_HOME:-~/.config}/fish/config.fish`; bash `~/.bashrc` on
  Linux. On macOS, terminals start bash as a login shell, so the first
  existing of `~/.bash_profile`, `~/.bash_login` is used (created as
  `~/.bash_profile` when neither exists); if bash would read `~/.profile`
  instead, install stops and asks for `--rc PATH`.
- Exactly one block is added; a second install changes nothing, byte for
  byte. A changed binary path is updated inside the block.
- Before any change to an existing file, an owner-only (`0600`) copy is
  written next to it as `<rc>.sentinal-backup-<unix-ms>`.
- The file is replaced atomically through a same-directory temporary file with
  the original permission bits. A symlinked rc file is edited at its target so
  dotfile-manager links survive. On any failure the original is unchanged.
- Uninstall removes only the block and restores every other byte, including
  encoding and whether the file ended with a newline.
- CRLF files, and missing, repeated, edited, nested, or out-of-order markers
  are reported with a line number and the file is left untouched.
- Install refuses a world-writable rc file, rc directory, `sentinal` binary,
  or binary directory.

## Performance

Measured by `cargo test --release -p cli --test hook_benchmark -- --ignored
--nocapture`: prompt round-trip for `true` in a real interactive shell in a
PTY, hooked (reporting to a fake socket) versus the same rc with
`SENTINAL_DISABLE=1`; 50 warmup and 500 measured iterations. Gate: added
p95 < 5 ms.

| Date | OS | Shell | Added p50 | Added p95 | Added max |
|---|---|---|---:|---:|---:|
| 2026-09-28 | Darwin 27.0.0 arm64 | zsh 5.9 | 1.13 ms | 1.47 ms | 1.70 ms |
| 2026-09-28 | Darwin 27.0.0 arm64 | bash 3.2.57 | 2.34 ms | 2.55 ms | 3.16 ms |
| — | Darwin 27.0.0 arm64 | fish | not installed | | |
| — | Linux | zsh/bash/fish | not yet measured | | |

bash costs more because each Command reads `history 1` in a subshell.

## Troubleshooting

1. Run `sentinal hook <shell>` to inspect the expected loader and hook, and
   `sentinal auto status` to see whether auto-wrap applies here.
2. Set `SENTINAL_DISABLE=1` to bypass external reporting and auto-wrap.
3. Confirm the per-user local socket exists and is owned by the current UID
   (see `docs/SECURITY.md`), or set `SENTINAL_SOCKET`.
4. Re-run `sentinal hook install <shell>`; it must not duplicate the block.

The hook must never prevent a shell from starting. A broken or unavailable
Sentinal installation degrades to a normal shell.
