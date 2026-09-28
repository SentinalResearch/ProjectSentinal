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

Auto-wrap is off by default. When enabled, a new interactive terminal enters
an `AUTO_WRAP` / `FULL` watched Session. It skips nested Sessions, IDE task
runners, scp/rsync, non-interactive SSH, and any environment with
`SENTINAL_DISABLE=1`.

Auto-wrap is fail-open: if the daemon does not answer, the original shell is
started normally within 300 ms with no Sentinal output injected into the
prompt.

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

An explicit or safely detected log path upgrades capture to `PROCESS_LOG`.
The tail is read-only and handles append, partial lines, truncation, rotation,
temporary absence, and bounded buffering. Lines enter the same deterministic
watcher pipeline as PTY output. stdout/stderr discovery runs off the prompt
hot path; Linux may inspect `/proc`, while macOS may use a cached `lsof`
probe.

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
