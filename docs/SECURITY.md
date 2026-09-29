# Security and privacy

This document separates protections implemented today from Milestone 1b
requirements. A planned guarantee is not considered active until its tests
and roadmap checkbox are complete.

## Current remote boundary

- The daemon accepts only loopback or Tailscale addresses and rejects
  `0.0.0.0`, `::`, and ordinary LAN addresses.
- `/api/v1/health` is public; every other HTTP or WebSocket route requires a
  bearer credential.
- The Machine token is generated from 256 random bits, stored mode `0600`,
  compared without early exit, and retained for CLI/bootstrap access.
- One owner password is stored only as an Argon2id hash. Browser sessions
  expire, logout and password change revoke them, and password attempts are
  rate-limited per client address.
- Every remote terminal input frame appends timestamp, client address,
  Session ID, and byte count to `audit.jsonl`. The input payload is excluded.
- Finished FULL Sessions reject input, resize, signal, and kill before a PTY
  handle or input audit entry is touched. A closed-device race is reconciled
  once and returned as a stable Session error, never a raw OS `EIO`.
- PTY output is retained in compressed Session logs. Terminal output is
  untrusted and is never executed.

`just network` is the combined development path for remote browser testing.
`just agent` binds only the worker daemon; `just control WORKER_IP=...` binds
only Vite and proxies HTTP/WebSocket traffic to that one worker. Every role
asks Tailscale for its Machine's IPv4 address, validates all addresses are in
`100.64.0.0/10`, rejects occupied/malformed ports, and never falls back to a
wildcard, ordinary LAN, fixtures, or localhost. Same-origin Vite proxying
keeps browser credentials out of frontend configuration. These launchers do
not bypass Tailnet ACLs or the host firewall and are not public-internet
deployment mechanisms.

The release installer likewise requires an exact validated Tailscale bind.
It accepts only current-user-owned, non-group/world-writable source binaries,
uses atomic program replacement, creates owner-only config/data directories,
and renders service files containing paths but no token or password hash.
Reinstall preserves credentials and user data. Uninstall retains them by
default; data deletion requires the exact resolved data path. Operational
commands and backup boundaries are documented in `docs/SETUP.md`.

## Planned package and enrollment boundaries

Milestone 1c.1 package delivery and Milestone 4.0 enrollment are intentionally
separate security ceremonies:

| Credential/state | Location | Purpose |
|---|---|---|
| Owner password hash | control Machine | Human login to the local control UI |
| Machine token | each worker only | Local daemon bootstrap/CLI boundary; never enrollment |
| Pairing code | hash on control; plaintext shown once | Short-lived, single-use connect authority |
| Machine credential | one worker + control registry | Unique revocable post-pairing identity |
| Browser Session | browser + control Machine | Owner UI access; contains no worker credential |

The private package URL contains none of these secrets. Install/reinstall
leaves a worker `UNPAIRED`. `sentinal connect` reads the code through a hidden
TTY prompt or bounded stdin; there is deliberately no pairing-code argv,
query-string, environment, or service-file form. A stable Machine ID is
authorization-neutral and separate from the editable display name.

The control role remains owner-operated inside Tailscale. It may serve
verified packages and hold the Machine registry, but it is not a required
vendor cloud. Exact code entropy, credentials, origin checks, transport, and
recovery remain subject to the mandatory Milestone 4.0 design approval.

## Data Sentinal records

| Data | Stored | Notes |
|---|---:|---|
| Terminal output from FULL Sessions | yes | compressed append-only Session log |
| Exact Command text and cwd | yes | structured SQLite history |
| Command exit and duration | yes | structured SQLite history |
| Machine/Session Metrics | yes | raw retention then minute aggregates |
| Remote input metadata | yes | audit JSONL, without payload bytes |
| Raw keystroke payload as its own log | no | must remain absent |
| Password plaintext | no | only Argon2id hash is stored |
| Explicit adopted log content | planned | PROCESS_LOG only, read-only source |

Commands may appear in PTY output because an interactive shell echoes them.
Hidden prompts such as password entry disable echo and therefore do not
appear in the output log. No component may add a separate keystroke recorder.

## Milestone 1b local boundary

External shell reporting, auto-wrap, and adoption use a private per-user Unix
socket. The implementation must:

- create the socket with mode `0600` in a user-owned directory;
- reject a peer whose OS-reported UID differs from the daemon UID;
- use bounded, versioned messages and recover from malformed messages;
- remove only a verified stale socket owned by the same user;
- refuse symlinks anywhere in the configured path, foreign-owned private
  parents, and non-sticky group/world-writable ancestors (macOS's fixed
  `/var` and `/tmp` system aliases are the only trusted aliases);
- expose no terminal-input operation on the local reporting protocol;
- keep hook reporting silent and asynchronous when unavailable.

Implemented in 1b.0 (`crates/daemon/src/local_socket.rs`):

- Location: Linux `$XDG_RUNTIME_DIR/sentinal/sentinald.sock` when that
  variable is an absolute path; macOS, and Linux without it,
  `<data_dir>/run/sentinald.sock`. `local_socket = "..."` in `daemon.toml`
  overrides both. Paths over 100 bytes are refused (portable `sun_path`).
- The expected UID is the daemon's own effective UID, read from a
  socketpair's OS peer credentials (no `unsafe`, no new dependency).
- Framing: 4-byte big-endian length, then 1..=65,536 bytes of versioned JSON
  `LocalRequest`; one request and one `LocalResponse` per connection. A
  wrong peer UID is answered `FORBIDDEN` before the frame is read and is
  recorded in `<data_dir>/security-audit.jsonl` without payload bytes.
- 1b.B handles `REGISTER_SHELL`, `SHELL_COMMAND_STARTED`, and
  `SHELL_COMMAND_FINISHED`; 1b.C handles read-only `ADOPT_PROCESS`. No local
  request can attach, resize, send input, or signal.
- A shell can be registered only if its PID exists and is owned by the
  daemon's UID; reports must come for that Session's own PID. Session
  identity is PID plus OS start time, so a reused PID never inherits a
  Session.
- COMMANDS Sessions store Command text, cwd, times, and exit codes only: no
  terminal log, screen, input audit, or environment. The hook passes Command
  text to its reporter through the environment rather than argv, so it is
  not visible to other users in `ps`.

Inspect a running daemon (default macOS path shown):

```sh
stat -f '%Sp %Su %N' ~/.local/share/terminal-agent/run ~/.local/share/terminal-agent/run/sentinald.sock   # macOS
stat -c '%A %U %n' "$XDG_RUNTIME_DIR/sentinal" "$XDG_RUNTIME_DIR/sentinal/sentinald.sock"                  # Linux
lsof -a -U -p "$(pgrep -x tad)" | grep sentinald   # the daemon holds the listener
ss -xlp | grep sentinald                           # Linux alternative
```

Expected: directory `drwx------`, socket `srw-------`, both owned by you.

Watch is an interactive CLI action and fails closed: if the daemon cannot
create the FULL Session, it prints a clear error and does not execute the
requested command. Shell hooks and auto-wrap fail open: hooks silently do
nothing, and auto-wrap starts the original shell within 300 ms.

## Watch initial Command (1b.A)

`sentinal watch -- COMMAND [ARG...]` sends a structured argv. The daemon, not
the CLI, turns it into shell input, and only through
`protocol::render_command_line`:

- every argument is a single quoted word; the executable is always quoted,
  so aliases, reserved words, and `NAME=value` assignments cannot change the
  program; zsh `=word` expansion and bash history expansion never apply;
- the line is typed into an interactive line editor, so control bytes
  (Ctrl-C, Ctrl-U, Tab, ESC, CR/LF, C1 controls) are written as `$'\xHH'`
  (bash/zsh) or `\xHH`/`\uHHHH` (fish) escapes, never as raw keystrokes;
  bash/zsh also escape non-ASCII bytes so readline meta-conversion cannot
  turn them into editor commands;
- the rendered line is capped at 1,000 bytes, below the 1,024-byte canonical
  input limit on macOS, so an early write can never be truncated into a
  different command;
- it is typed exactly once, after the first `OSC 133;A` prompt report, and
  only for `WATCH` Sessions whose shell has Sentinal hooks. Terminal output
  never contributes to what is typed.

The CLI never executes the Command itself; every failure path (nested,
not a terminal, daemon unavailable, rejected) exits non-zero before or
instead of creating a Session.

## Auto-wrap and adoption (1b.C)

- Auto-wrap never execs or relaunches a shell: the rc block runs a child and
  continues on anything but success, so a broken or absent daemon cannot
  prevent a shell from starting. Recursion is stopped by
  `SENTINAL_SESSION_ID` and the explicit `SENTINAL_AUTO_GUARD=1` that the
  daemon sets in every AUTO_WRAP shell; a reply not marked `AUTO_WRAP`
  (an older daemon) is refused. Creation and receipt of the initial screen
  share one 200 ms deadline. The client acknowledges that screen with its
  first resize; without that acknowledgement the daemon interrupts the
  AUTO_WRAP Session after 300 ms.
- Adoption is local-only (`ADOPT_PROCESS` over the private socket) and
  read-only: the daemon's `ProcessProbe` exposes only identity, metadata,
  and log-discovery reads; it has no signal, write, ptrace, priority, or
  kill method. Adopted Sessions are PROCESS capture, so attach, input,
  resize, and kill are refused even for handcrafted remote requests.
- Only processes owned by the daemon's UID can be adopted, never
  `sentinald` itself; identity is PID + OS start time.
- An explicit adopted log must be an absolute, readable regular file owned
  by the daemon UID. The daemon records only its path until 1b.D enables
  read-only tailing.
- Log discovery reads `/proc/<pid>/fd` links or runs `lsof` with a 2 s
  timeout, off the request path; it opens nothing belonging to the process.

## Capture and lifecycle capability enforcement

The daemon—not the UI—maps capture levels to allowed operations:

| Operation | FULL | COMMANDS | PROCESS_LOG | PROCESS |
|---|---:|---:|---:|---:|
| Read screen / attach | yes | no | no | no |
| Send terminal input | yes | no | no | no |
| Resize PTY | yes | no | no | no |
| Observe Commands | yes | yes | no | no |
| Observe Metrics | yes | yes | yes | yes |
| Read configured log | n/a | no | yes | no |
| Signal adopted process | no adoption | no | no | no |

Adoption is same-user and read-only. It does not signal, write to, debug,
reparent, or claim an exit code for a process Sentinal did not start.

Capture permission is necessary but not sufficient for PTY mutation. The
daemon applies one state-aware operation gate after the capture check:
`COMPLETED`, `FAILED`, and `INTERRUPTED` can still expose retained output but
cannot accept input, resize, signal, or kill. Rejected input is absent from
the input audit, SQLite, and terminal log.

Saved FULL output is exposed only through authenticated
`GET /api/v1/sessions/{numeric-id}/output`. The daemon derives the path from
its configured data directory and the parsed numeric ID; cwd, title, adopted
log paths, and caller paths are never used. Responses preserve ANSI bytes,
are limited to 1 MiB, and scan at most 64 MiB of decompressed data per
request. Corrupt/truncated final frames return preceding output with a stable
warning. The web writes these bytes to xterm, not `innerHTML`, and Continue
uses structured safe metadata to create a new Session without copying output
or an old Command into a shell.

## Intelligence permissions

The intelligence service is an ordinary authenticated client. Initial levels
are observe, summarize, and suggest (Levels 0–2). It cannot adopt or signal a
process. Terminal input is allowed only for a future explicitly authorized
action on a FULL Session; model output alone never grants that authorization.

Terminal output and tailed logs are untrusted data, not instructions. They
must not be interpolated into a shell command or treated as a policy source.

## Security release checks

- Binding and authentication behavior tests pass.
- Local socket mode and peer UID tests pass on Linux and macOS.
- Wrong-user, malformed, oversized, stale-socket, and symlink cases fail
  safely.
- Every reduced capture level is rejected by attach/input/resize/kill tests.
- Every terminal FULL state is rejected by input/resize/kill tests before fake
  PTY counters or the input audit change; live detach/reattach stays writable.
- Authenticated replay covers restart, pagination bounds, empty/missing/
  corrupt logs, unknown Sessions, and traversal attempts.
- Storage inspection finds no raw input payload in SQLite, audit JSONL, or a
  separate file.
- Hook/auto-wrap daemon-down tests prove the documented fail behavior.
