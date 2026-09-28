# MILESTONE 1b.A — Sentinal Watch

Goal: from any ordinary terminal, run

```console
sentinal watch -- python long_job.py
```

then close the window and reattach to the same process and screen from another
terminal or the web UI.

Depends on completed Milestone 1b.0. Re-read the approved protocol fixtures
and `docs/CAPTURE_MODES.md`; do not redesign them in this stage.

## 1. Product binary names and compatibility

- Add installed user-facing binaries `sentinal` and `sentinald`.
- Keep `ta` and `tad` as compatibility aliases for the window approved in
  Milestone 1b.0; do not silently remove service compatibility.
- Help, errors, and new documentation use Sentinal names.
- Preserve current `ls`, `new`, `attach`, and `kill` behavior.

## 2. Watch CLI

Implement:

```console
sentinal watch
sentinal watch -- COMMAND [ARG...]
sentinal watch -t TITLE -- COMMAND [ARG...]
```

- Use the current directory and current terminal dimensions.
- Ask `sentinald` to create a `WATCH` / `FULL` Session and immediately attach
  the current terminal with the existing detach sequence (`Ctrl-\`, then `d`).
- With no Command, start the user's normal interactive shell.
- With a Command, preserve the approved structured representation exactly,
  execute it once after shell readiness, and keep the shell open afterward.
- Render any shell text only from the approved structured arguments using the
  protocol shell-quoting helper. Never concatenate unquoted arguments and
  never execute text derived from terminal output.
- Set `SENTINAL_SESSION_ID` inside the daemon-owned Session before user shell
  configuration loads.
- If `SENTINAL_SESSION_ID` is already set in the calling environment, create
  no Session and execute no supplied Command. Report that the terminal is
  already watched and return the documented success/error code.
- If the daemon is unavailable or rejects creation, print one clear error,
  return non-zero, and never execute the supplied Command outside Sentinal.
- EOF, client crash, or window close detaches only. It must not kill the shell
  or child process.

## 3. Daemon behavior

- Extend Session creation only as approved in 1b.0.
- Do not overload ordinary UI creation with Watch-specific environment or
  initial-Command behavior.
- Track the shell root PID in the Session and process Metrics.
- Initial Command start/finish is captured by the existing OSC tracker and
  stored exactly once.
- Reattach sends the current screen before live bytes, as in Milestone 1.
- Watch Sessions remain authorized by capture capabilities rather than origin
  conditionals scattered through handlers.

## Tests first

- CLI parse: bare Watch, title, `--`, empty argv rejection, arguments with
  whitespace/quotes/metacharacters/Unicode, and current cwd/dimensions.
- Quoting/injection: an argv containing `$(...)`, semicolons, newlines, single
  quotes, and empty arguments reaches the fake PTY as data and executes only
  the intended program/arguments.
- No nesting: set `SENTINAL_SESSION_ID`; assert zero create requests and zero
  Command execution.
- Daemon unavailable/rejected: clear error, non-zero status, supplied Command
  never runs.
- Fake PTY behavior: origin/capture/title/cwd/PID are correct; initial Command
  runs once; shell remains IDLE afterward; two attach clients receive output;
  detach leaves it running.
- Restart/history: Watch Session and initial Command persist through SQLite in
  the approved shape.
- Integration `watch_integration` (`#[ignore]`): run a real long job, close or
  detach the first CLI, attach with a second CLI, observe the same PID/screen,
  then clean up explicitly.
- Playwright: Watch-created Session appears with FULL capture and can attach.

Tests may not use sleeps over 100 ms. Use fake readiness signals and channels
for the fast suite.

## Done when

The Watch demo succeeds by CLI and web on the available host, `just check`
passes, the ignored integration is run, CLI/capture/security docs are updated,
and Roadmap 1b.A is checked. Report exact commands, compatibility aliases,
test counts, and remaining platform coverage. Commit and stop before 1b.B.
