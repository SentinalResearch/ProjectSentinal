# Shell hooks

Sentinel has two shell-integration paths. They share Command semantics but
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

## Ordinary-shell socket path (Milestone 1b, planned)

`sentinel hook install` adds a small loader to the user's rc file. It
registers the shell and reports Command start/finish, cwd, duration, exit
code, and shell PID to the private local Unix socket. It does not send output
or keystrokes.

If `SENTINEL_SESSION_ID` is set, the socket reporter is disabled and the
existing OSC path remains active. This is the no-double-counting rule.

If `SENTINEL_DISABLE=1` is set, all external reporting and auto-wrap are
disabled. The daemon-owned OSC wrapper may still track the FULL Session that
the user explicitly entered.

## Install and uninstall contract (planned)

- Detect zsh, bash, or fish explicitly; do not guess when ambiguous.
- Create a timestamped backup before the first edit.
- Add exactly one marker-delimited Sentinel block.
- Installing twice produces an identical file after the first install.
- Uninstall removes only that block and restores the original surrounding
  bytes exactly, including final newline behavior.
- Never source a file from a world-writable location.
- A malformed or partially edited marker block is reported and left intact
  for manual review.

## Failure and performance contract (planned)

- Reporting runs off the prompt path with a 50 ms transport deadline.
- Daemon unavailable, socket missing, timeout, or malformed reply produces no
  prompt output and does not alter the Command's exit status.
- Measured hook overhead must be below 5 ms at p95. The milestone report
  records platform, shell versions, iterations, p50, p95, and maximum.
- Fast tests use a fake socket and injected clock. Real zsh, bash, and fish
  run manually on Linux and macOS before release.

## Troubleshooting (target behavior)

1. Run `sentinel auto status` and `sentinel hook install --help` to identify
   the configured integration.
2. Set `SENTINEL_DISABLE=1` to bypass external reporting and auto-wrap.
3. Confirm the per-user local socket exists and is owned by the current UID.
4. Run `sentinel hook <shell>` to inspect the expected loader.
5. Re-run `sentinel hook install <shell>`; it must not duplicate the block.

The hook must never prevent a shell from starting. A broken or unavailable
Sentinel installation degrades to a normal shell.
