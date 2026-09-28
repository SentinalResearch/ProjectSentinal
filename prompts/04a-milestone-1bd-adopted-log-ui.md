# MILESTONE 1b.D — Adopted log tail and capture-aware UI

Goal: an adopted process with a log becomes `PROCESS_LOG`, its lines flow
through the deterministic watcher pipeline, and every Session surface clearly
shows what Sentinal can and cannot do.

Depends on completed Milestone 1b.C and Milestone 3 Event engine/watchers.
Do not duplicate watcher parsing in the daemon or web app.

## 1. Read-only log tail

- Tail only an explicit or approved discovered regular-file path associated
  with an adopted Session.
- Open read-only; never create, truncate, chmod, rename, lock, or write.
- Track file identity and offset so append, partial lines, truncation, rename
  rotation, copy-truncate rotation, temporary disappearance, and recreation
  are handled deterministically.
- Do not replay the entire historical file by default. Define and document the
  bounded initial-tail policy (bytes/lines) and persist enough cursor metadata
  to avoid surprise duplication after daemon restart.
- Bound unread bytes, line length, memory, and event rate. Over-limit data
  produces a structured warning and resumes safely rather than growing
  without limit.
- Decode bytes using the watcher pipeline's approved lossy/encoding policy;
  preserve raw files untouched.
- Stop tailing when the adopted process and its descendants have ended, after
  draining already-readable final bytes. Never infer an exit code from log
  text.
- If the path becomes unsafe (symlink/owner/type change according to the
  approved policy), stop and emit a warning instead of following it.

## 2. Watcher integration

- Feed complete and carriage-return-delimited lines as normal output
  Observations into the existing generic and matched adapters.
- Store and broadcast resulting Events through the same engine as FULL
  Sessions.
- Do not treat a log line as a Command or an instruction.
- Upgrade capture to `PROCESS_LOG` only after the tail is successfully active;
  fall back to `PROCESS` with a structured warning when it cannot continue.
- Notifications follow the capture evidence matrix. An adopted process may
  notify “ended” but not “failed” unless independent structured evidence—not
  guessed exit text—supports it.

## 3. Capture-aware web UI

- Show an origin/capture badge on Overview, Session detail, Project/Split
  surfaces, History associations, and any Session selector.
- Provide one concise explanation:
  - FULL: terminal and Commands captured;
  - COMMANDS: Commands/Metrics only, no terminal;
  - PROCESS_LOG: read-only log/Metrics, no terminal input;
  - PROCESS: Metrics only.
- Hide or disable attach, terminal input, resize, Ctrl-C, and kill when the
  daemon capability disallows them. Never rely on hiding alone; daemon checks
  remain authoritative.
- For reduced capture, replace an empty terminal with the appropriate Command
  view, read-only watched-log/events view, or Metrics view.
- Show “Watch fully next time” with a copyable `sentinal watch ...` hint when
  enough safe metadata exists. Do not synthesize an unsafe shell command from
  arbitrary output.
- Ensure phone layouts expose the badge and limitation without opening a
  secondary panel.

## Tests first

- Tail unit/behavior: initial bounded tail, append, partial line then append,
  CR progress, truncate, copy-truncate, rename rotation, missing/recreated
  file, same-size replacement, final drain, long line, byte/memory limit,
  invalid UTF-8, permission loss, symlink/type/owner change.
- Restart: cursor persistence avoids whole-file replay and handles identity
  mismatch safely.
- Watcher behavior: adopted training/pytest/server log fixtures produce the
  same normalized Events as equivalent FULL Session observations.
- Capture transitions: activate → PROCESS_LOG; tail failure → PROCESS plus
  warning; process end drains and stops without fabricated exit code.
- Notifications: evidence matrix prevents unsupported failure/progress claims.
- Web Testing Library: every origin/capture badge, explanation, replacement
  panel, forbidden-action absence, and safe Watch hint.
- API security: handcrafted attach/input/resize/kill remains rejected for
  PROCESS_LOG and PROCESS Sessions.
- Playwright phone viewport: adopted log Session shows progress/events and no
  interactive terminal controls.
- Integration `log_tail_integration` (`#[ignore]`): real process writes,
  truncates, and rotates a temp log while daemon remains attached.

No sleeps over 100 ms in fast tests; drive file changes and watcher ticks with
injected notifications/clocks.

## Done when

Adopted log progress works end to end, all four modes display accurate
capabilities, the full Milestone 1b release gates and Linux/macOS matrix are
recorded, `just check` and Playwright pass, and Roadmap 1b.D/release gates are
checked. Commit, send the Milestone 1b.D report, and stop before Milestone 4.
