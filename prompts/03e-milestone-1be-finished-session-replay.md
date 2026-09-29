# MILESTONE 1b.E — Finished Session lifecycle and output replay

Goal: a finished FULL Session is a safe read-only record. It never accepts
bytes for a closed PTY, never displays a raw OS `EIO`, and can be continued
only by creating a distinct new Session.

This is a focused pre-Milestone-3 correctness slice. Read `AGENTS.md`,
`docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/CLI.md`, and the current
Session/capture protocol fixtures before changing code.

## Required semantics

- Detaching from a live FULL Session keeps it writable and reattachable.
- `COMPLETED`, `FAILED`, and `INTERRUPTED` are terminal states. Their retained
  screen/output may be read, but input, resize, signal, and kill are rejected
  before a PTY handle or input audit log is touched.
- A graceful shell exit remains `COMPLETED` for code 0 and `FAILED` otherwise.
  A daemon kill remains `INTERRUPTED`.
- “Continue” never revives a process or reuses a Session ID. It creates a new
  `UI` / `FULL` Session using only the old Session's safe shell, cwd, dimensions
  when known, and a clearly derived title. It never re-executes an old Command.
- Saved terminal output is untrusted data. It is displayed as terminal bytes
  or downloaded as data; it is never interpolated into HTML or a shell.

## 1. Daemon lifecycle gates

- Centralize a state-aware PTY operation check used by input, resize, kill,
  and future signal paths in addition to the existing capture check.
- Keep read-only attach/snapshot available for a finished Session while its
  in-memory handle exists.
- Return a bounded structured error naming the Session ID, terminal state,
  and rejected operation. Do not expose `Input/output error (os error 5)` to
  clients for a known terminal state.
- If a PTY write reports a closed-device error before the exit monitor has
  observed termination, reconcile child state once, return a stable error,
  and avoid repeated writes. Do not guess an exit code.
- Rejected input must not create a remote-input audit entry because no input
  reached the PTY. Security rejection metadata may be logged separately
  without payload bytes.

## 2. Persisted output replay and export

- Add an authenticated, bounded way to read output from
  `<data_dir>/sessions/<id>.log.zst`, including Sessions loaded from SQLite
  after a daemon restart.
- Before implementation, write the concrete HTTP/API proposal in
  `docs/progress/milestone-1be.md`: route, authentication, content type,
  pagination/range policy, decompressed byte limit, missing/corrupt log errors,
  and whether ANSI preview and raw compressed download are separate routes.
- Prefer an additive HTTP resource over loading an unbounded log into a
  WebSocket control message. Keep `PROTOCOL_VERSION = 1` if no existing wire
  JSON changes.
- Stream/decompress with bounded memory. Never return arbitrary filesystem
  paths, follow a Session-supplied path, or permit path traversal.
- A corrupt/truncated final zstd frame returns all verified preceding output
  plus a visible structured warning; it must not crash the daemon.
- The UI may replay ANSI bytes through xterm. A plain-text export, if included,
  must have a specified ANSI/control-character policy and tests.

Stop and ask the human before implementation if the proposed replay API
changes an existing fixture, requires a dependency, or cannot be bounded.

## 3. Web lifecycle UX

- Finished Sessions open in a visibly read-only terminal/output view.
- Disable xterm input registration, resize requests, mobile keys, Ctrl-C, and
  kill for terminal states. The daemon remains authoritative.
- Show `Session ended — output saved` with state, exit code when known, and
  the output retention location concept without exposing a server path.
- Add “Continue in new Session”. On success navigate to the new Session and
  make the distinct ID visible. On failure leave the old record unchanged.
- Closing a pane remains detach/view cleanup, never Session termination.
- A live detached Session must not be mislabeled read-only.

## Tests first

- Daemon: finished `COMPLETED`, `FAILED`, and `INTERRUPTED` Sessions reject
  input/resize/kill without calling fake PTY operations.
- Privacy: rejected bytes appear in neither `audit.jsonl`, SQLite, nor logs.
- Race: child exit concurrent with one final input produces one stable state
  and no repeated raw `EIO` response.
- Live detach/reattach remains writable.
- Replay: valid multi-frame log, empty log, missing log, bounded large log,
  corrupt/truncated final frame, unknown Session, unauthorized request, and
  path-traversal attempts.
- Restart: a finished SQLite Session whose PTY handle is absent can still
  replay its persisted output.
- Web: terminal-state matrix is read-only; live-state matrix stays interactive;
  Continue creates one new Session and never resends an old Command.
- Playwright: finish a fake-backed Session, verify no input frame is sent,
  replay output, continue to a new ID, and attach to the new Session.

No real sleeps over 100 ms and no network access in fast tests.

## Done when

The reported Sessions that previously produced OS error 5 are read-only and
replayable, live reattach still accepts input, Continue creates a distinct
Session, `just check` and Playwright pass, Roadmap 1b.E and progress docs are
updated, and the report includes exact replay/export commands. Commit and stop
before Milestone 1c.
