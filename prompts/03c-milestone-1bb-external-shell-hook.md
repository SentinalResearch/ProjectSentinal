# MILESTONE 1b.B — External shell hook

Goal: an ordinary zsh, bash, or fish terminal records Commands and Metrics as
a `SHELL_HOOK` / `COMMANDS` Session without capturing output, changing prompt
behavior, or requiring the daemon to own its PTY.

Depends on completed 1b.0 local ingress and the existing Milestone 2 OSC
tracker. This stage adds the ordinary-shell socket path; it must not replace
or duplicate the daemon-owned OSC path.

## 1. CLI and generated integrations

Implement:

```console
sentinal hook zsh|bash|fish
sentinal hook install [zsh|bash|fish]
sentinal hook uninstall [zsh|bash|fish]
```

- `hook <shell>` prints the external integration to stdout without editing a
  file.
- `install` selects only an unambiguous supported shell/config path or uses
  the explicit shell argument.
- Add one stable marker-delimited Sentinal block. A second install is
  byte-for-byte idempotent.
- Before the first change, create a timestamped owner-only backup.
- `uninstall` removes only the exact Sentinal block and restores all other
  bytes, encoding, final newline state, permissions, and unrelated edits.
- Malformed/nested/partial markers produce a clear error and no file change.
- Use atomic same-directory replacement with preserved permissions.
- Never source hook code from a world-writable file or directory.

## 2. Runtime reporting

- On interactive shell start, register the approved shell PID, executable,
  cwd, and optional title through the private local socket.
- Store a `SHELL_HOOK` / `COMMANDS` Session rooted at that shell PID.
- Report exact Command text, cwd, start, finish, duration, and exit code using
  the approved local request fields.
- Preserve the Command's real exit status across all hook bookkeeping.
- Reporting runs in the background/off the prompt path with a 50 ms transport
  deadline. Socket missing, daemon down, timeout, or bad reply is completely
  silent in the terminal.
- If `SENTINAL_SESSION_ID` or `SENTINAL_DISABLE=1` is set, make no external
  registration or Command socket calls.
- Do not send terminal output, stdin, environment contents, or raw keystrokes.
- Keep existing daemon-owned zsh/bash/fish OSC hooks working unchanged.

## 3. Lifecycle and Metrics

- Use the shell PID as the Session process-tree root.
- Feed its process tree into the existing Session Metrics sampler.
- Monitor PID identity/liveness without hot-looping and without confusing PID
  reuse for the original shell. Use the termination representation approved
  in 1b.0.
- A COMMANDS Session cannot attach, accept input, resize, or be killed through
  Sentinal. The web UI may list it but capability enforcement remains daemon
  side.
- Daemon restart may recover a still-live registered shell only if identity
  can be verified safely; otherwise mark it with the approved disconnected or
  ended semantics and document the limitation.

## 4. Performance measurement

- Add a reproducible hook benchmark using a fake local socket responder.
- Record shell version, OS, iterations, warmup, p50, p95, and maximum added
  prompt time.
- The release gate is p95 below 5 ms for each available shell.
- Benchmark code is ignored/manual if timing would make the fast suite flaky;
  deterministic deadline behavior still has fast injected-clock tests.

## Tests first

- Installer fixtures for empty/missing/existing rc files, no final newline,
  CRLF where applicable, existing user content, duplicate install, exact
  uninstall, malformed markers, permissions, and failed atomic replacement.
- Script tests run each hook against a fake socket server and assert register,
  start, finish, cwd, duration, exit code, and preservation of `$?`/`$status`.
- Suppression tests for `SENTINAL_SESSION_ID` and `SENTINAL_DISABLE=1` assert
  zero socket messages.
- Daemon-down, timeout, and malformed-reply tests assert no stdout/stderr and
  an unaffected prompt/exit code.
- Daemon behavior: registration creates one COMMANDS Session; repeated shell
  registration is idempotent; Commands store once; zero-output Commands count;
  Metrics use the shell process tree; forbidden PTY operations return Error.
- Lifecycle: fake process identity alive, exited, and PID-reused cases.
- Integration `external_shell_hook_integration` (`#[ignore]`): real zsh,
  bash, and fish execute `true`, `false`, `cd`, and an argument containing
  metacharacters against a real local daemon.
- Privacy test inspects socket capture, SQLite, audit JSONL, and logs: no
  output/keystroke payload was stored for the COMMANDS Session.

No fast test may rely on an installed shell or sleep over 100 ms.

## Done when

Install/uninstall and daemon-down behavior work by hand; measured p95 is below
5 ms on available shells; `just check` passes; integration results and missing
platform rows are reported; docs and Roadmap 1b.B are updated. Commit and stop
before 1b.C.
