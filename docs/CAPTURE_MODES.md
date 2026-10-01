# Capture modes

This is the normative product specification for bringing work into Sentinal.
It describes target Milestone 1b behavior; fields and requests explicitly
marked “planned” do not exist in protocol version 1 yet.

## Session origin and capture

Every Session records how it entered Sentinal and what the daemon can see.

| Origin | Default capture | Meaning |
|---|---|---|
| `UI` | `FULL` | Created through the web/API/ordinary new-Session flow |
| `WATCH` | `FULL` | Created by `sentinal watch` from an ordinary terminal |
| `SHELL_HOOK` | `COMMANDS` | Ordinary shell reporting structured Command boundaries |
| `AUTO_WRAP` | `FULL` | Interactive shell automatically moved into a persistent PTY |
| `ADOPTED` | `PROCESS` or `PROCESS_LOG` | Existing same-user process observed read-only |

| Capture | Output/input | Commands | Metrics | Remote attach | Survives source window |
|---|---:|---:|---:|---:|---:|
| `FULL` | yes | yes | yes | yes | yes |
| `COMMANDS` | no | yes | yes | no | no |
| `PROCESS_LOG` | read-only log | no | yes | no | process decides |
| `PROCESS` | no | no | yes | no | process decides |

The daemon derives capabilities from capture. Clients must never infer
authorization from origin alone. Every Session surface shows a capture badge
and a concise explanation of unavailable actions.

Notification evidence is capture-dependent:

| Capture | Available notification evidence |
|---|---|
| `FULL` / `PROCESS_LOG` | finished, failed where known, waiting, stalled, errors, progress |
| `COMMANDS` | Command finished/failed and resource issues |
| `PROCESS` | process ended and resource issues |

## Mode 1 — Watch

```console
$ sentinal watch
$ sentinal watch -- python train.py
$ sentinal watch -t "Qwen Training" -- python train.py
```

The CLI asks the daemon for a `WATCH` / `FULL` Session in the caller's cwd,
at the caller's terminal size, then attaches in the current terminal. With a
command, the daemon types it once at the shell's first prompt, rendered from
the structured argv by `protocol::render_command_line`, and leaves the shell
available afterward. Closing the client detaches but does not terminate the
Session. `Session.pid` is the shell (process-tree root) and feeds process
Metrics; the initial Command is captured by the ordinary OSC tracker and is
stored once, like any typed Command.

`SENTINAL_SESSION_ID` is set inside every daemon-owned FULL Session (UI and
Watch) before user shell configuration loads. Invoking Watch when it is
already set creates no Session: a bare Watch exits 0, and a Watch with a
Command exits 1 without running it. If the daemon is unavailable or rejects
creation, Watch exits 1 and never runs the supplied command outside Sentinal.

Wire representation (approved in 1b.0): `CREATE_SESSION.command` is a
structured `Vec<String>` argv. Initial Commands are accepted only for
`WATCH`, only with zsh/bash/fish (whose hooks report readiness), and only
when the rendered line is at most 1,000 bytes. See `docs/CLI.md`.

## Mode 2 — external shell hook

```console
$ sentinal hook install
```

Install adds one marker-delimited integration block to zsh, bash, or fish
configuration. An ordinary interactive shell registers a `SHELL_HOOK` /
`COMMANDS` Session and reports exact Command text, cwd, start, finish,
duration, exit code, and shell PID through the private local socket.

Reporting is asynchronous, silent when the daemon is unavailable, and uses a
50 ms transport deadline. It must add less than 5 ms p95 overhead at the
prompt. When `SENTINAL_SESSION_ID` is set, the external socket path is
suppressed because daemon-owned PTYs already report OSC signals.

The shell PID is the Session's process-tree root for Metrics and lifecycle.
Closing the ordinary terminal ends the Session (`COMPLETED`, exit code
unknown, checked every 2 s by PID and OS start time); output and remote input
are unavailable. Implemented in 1b.B; details and measured overhead are in
`docs/SHELL_HOOKS.md`.

## Mode 3 — auto-wrap

```console
$ sentinal auto on
$ sentinal auto off
$ sentinal auto status
```

Auto-wrap is off by default. `auto on` adds a marker-delimited block at the
top of the shell's rc file (same editor, backup, and atomic replace as
`hook install`). In a new interactive terminal the block runs
`sentinal auto enter` as a child of the starting shell; it creates an
`AUTO_WRAP` / `FULL` Session through the Watch path and attaches. When that
Session's shell exits (or you detach), the outer shell exits too.

Skip rules (first match wins; one table in `crates/cli/src/auto_wrap.rs`):

| Signal | Reason |
|---|---|
| `SENTINAL_DISABLE=1` | disabled |
| `SENTINAL_SESSION_ID` non-empty | already inside a Session |
| `SENTINAL_AUTO_GUARD` non-empty | recursion guard (set in every AUTO_WRAP shell) |
| shell not interactive | non-interactive shell |
| stdin or stdout not a tty | no terminal |
| `SSH_ORIGINAL_COMMAND` runs scp, rsync, sftp, or sftp-server | file transfer |
| `SSH_ORIGINAL_COMMAND` set, or `SSH_CONNECTION`/`SSH_CLIENT` without `SSH_TTY` | non-interactive SSH |
| `VSCODE_RESOLVING_ENVIRONMENT` or `INTELLIJ_ENVIRONMENT_READER` | IDE reading the login environment |
| any variable named in `SENTINAL_AUTO_SKIP` (comma-separated) is set | configured task runner |
| `TERM` unset or `dumb` | dumb terminal |
| not zsh, bash, or fish | unsupported shell |

Ordinary IDE terminals (`TERM_PROGRAM=vscode`, JetBrains JediTerm, Emacs
vterm) and interactive SSH are wrapped; there is no broad `TERM_PROGRAM`
rule.

Auto-wrap is fail-open: `auto enter` has 200 ms total to reach the daemon,
create the Session, and receive its initial screen. On any skip, timeout, or error it exits 75 silently and
the rc file simply continues, so the original shell keeps its own argv,
environment, cwd, and exit behavior; nothing is relaunched, which also rules
out exec loops. Measured worst case with a hung daemon: +233 ms (zsh), +223 ms
(bash) to shell start on macOS. A reply that arrives after the deadline is
dropped. An attachment commits only after the client receives the initial
screen and acknowledges it with a resize; the daemon interrupts an
unconfirmed AUTO_WRAP Session after 300 ms, so a late success cannot leave a
second shell. A daemon that does not mark the Session `AUTO_WRAP` is refused.

The wrapped shell is started by `sentinald`, so it gets the daemon's
environment plus `SENTINAL_SESSION_ID` and `SENTINAL_AUTO_GUARD=1`, not the
terminal's (same as Watch).

## Mode 4 — adopt

```console
$ sentinal adopt 4242
$ sentinal adopt 4242 --log ~/Projects/TIM/train.log
$ sentinal adopt 4242 -t "Qwen Training"
$ sentinal adopt --pick
```

Adopt registers an existing same-user PID as `ADOPTED` / `PROCESS`. The daemon
samples the process tree and determines whether it remains alive. Because
Sentinal is not the parent, completion is displayed as “Ended · exit code
unknown.” It must not manufacture success or failure.

An explicit `--log` path must be absolute, readable, a regular file, and
owned by the daemon user. Otherwise redirected stdout/stderr are detected
after adoption, off the request path (Linux `/proc/<pid>/fd/{1,2}`; macOS
`lsof -a -p PID -d 1,2 -Fftn` with a 2 s timeout). Pipes, sockets, devices,
deleted files, and non-regular files are ignored; failure leaves no active
tail and capture stays `PROCESS`.

An active log upgrades capture to `PROCESS_LOG`. The tail is read-only
and handles append, partial lines, truncation, rotation, temporary absence,
and bounded buffering. Lines enter the same deterministic watcher pipeline
as PTY output.

Adopt resolves the target before any change: the PID must be non-zero,
alive, owned by the daemon user, not `sentinald` itself, and not already
adopted (a reused PID ends the stale Session instead of inheriting it).
Name, executable, and cwd are recorded when available. Metrics and endings
of reduced-capture Sessions reach every web client without attaching.

Adoption never writes, signals, reparents, attaches a debugger, or opens the
process's input. A future explicit signal action requires a separate security
review and user confirmation; it is not implied by adoption.

## Local ingress

External hooks, auto-wrap, and adoption use a per-user Unix socket rather than
the remote HTTP/WebSocket listener. The socket:

- is owned by the daemon user and mode `0600`;
- verifies the peer UID before decoding a request;
- accepts bounded, versioned messages;
- cannot attach or send PTY input;
- reports errors to interactive CLI calls but remains silent for shell hooks;
- is removed safely when stale and never follows an untrusted symlink.

## Privacy and AI boundary

- Sentinal stores terminal output for FULL Sessions and explicit log content
  for PROCESS_LOG Sessions; it does not separately record raw keystrokes.
- Hidden password prompts do not echo and therefore are absent from terminal
  output logs.
- Audit records contain actor, Session, time, and byte count—not input bytes.
- Reduced-capture Sessions reject terminal input, resize, attach, and kill.
- The intelligence service cannot send input unless capture is FULL and can
  never adopt or signal a process.
- Terminal and log output are untrusted data, never executable instructions.

## Acceptance

Milestone 1b is complete only when all roadmap gates pass, including the
Linux/macOS shell matrix, p95 hook latency under 5 ms, 300 ms auto-wrap
fail-open, same-user adoption, rotation-safe log tail, capture-aware UI, and a
storage inspection proving raw input payloads are not persisted.
