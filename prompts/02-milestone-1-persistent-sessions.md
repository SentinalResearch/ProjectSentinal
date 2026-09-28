# MILESTONE 1 — Persistent sessions (v0.1)

Goal: start `python long_job.py`, close every client, reattach later from
another device and see the same process and the same screen.

## Crates to create

`crates/daemon` (binary `tad`) and `crates/cli` (binary `ta`), plus a
minimal `web/` page.

## Design

- **SessionManager** owns sessions: `HashMap<SessionId, SessionHandle>`.
  Each handle has: the `portable-pty` child + master, a `vt100::Parser`
  (current screen), a scrollback ring buffer (default 10 000 lines,
  configurable), and a `tokio::sync::broadcast` channel of output bytes for
  attached clients. One reader task per PTY feeds parser, ring buffer,
  on-disk log, and broadcast.
- Put PTY spawning behind a small trait `PtyBackend` with a real
  implementation and a `FakePty` for fast tests. This is the one justified
  trait (two real implementations).
- **On-disk log:** append raw bytes to
  `<data_dir>/sessions/<id>.log.zst` (zstd streaming). This becomes
  training data later; never truncate it silently.
- **Attach:** send current screen contents (`vt100` → ANSI redraw) as the
  first binary frame, then live bytes. Several clients may attach at once.
- **Detach / disconnect:** the session keeps running. Nothing is killed when
  the last client leaves.
- **Exit detection:** when the child exits, set state `COMPLETED` (code 0)
  or `FAILED`; on `KILL` set `INTERRUPTED`. Use `Session::transition`; never
  set `state` directly.
- **API (axum):**
  - `GET /api/v1/health` (no auth) → `{ "ok": true, "version": 1 }`
  - `GET /api/v1/machine`, `GET /api/v1/sessions`
  - `WS /api/v1/ws` — text frames carry `Envelope<ClientRequest>` /
    `Envelope<DaemonMessage>`; binary frames carry terminal bytes with a
    header: `[u64 session_id big-endian][payload]`. Input from client uses
    the same framing.
  - Validate every request with `ClientRequest::validate()`; reply with
    `DaemonMessage::Error` on failure; never drop the connection for a bad
    message.
- **Config:** `~/.config/terminal-agent/daemon.toml`: `bind` (default
  `127.0.0.1:7420`), `data_dir`, `scrollback_lines`, `token_file`.
  Refuse to start if `bind` is `0.0.0.0` or `::`.
- **Auth:** on first start, generate a random token into `token_file`
  (mode 0600). Require `Authorization: Bearer` (or `?token=` for WS from
  browsers). Compare in constant time.
- **Audit log:** `<data_dir>/audit.jsonl`, one line per remote input:
  timestamp, client address, session id, byte count.
- **Service files:** `packaging/systemd/tad.service` (user unit) and
  `packaging/launchd/dev.terminal-agent.tad.plist`.

## CLI (`ta`)

`ta ls`, `ta new [--cwd DIR]`, `ta attach <id>` (raw mode, detach with
`Ctrl-\ d`), `ta kill <id>`. Uses the same WebSocket API.

## Web (minimal)

Vite + React + xterm.js page: session list, click to attach, resize on
window resize, mobile key bar with Ctrl-C, Esc, Tab, ↑ ↓, `y`, `n`.
Token entered once, kept in memory (and optionally `localStorage`).

## Tests first

- Unit (FakePty): create → IDLE; output reaches two attached clients;
  detach leaves session running; kill → INTERRUPTED; child exit 0 →
  COMPLETED, exit 3 → FAILED; resize with 0 rejected; ring buffer keeps
  only the last N lines.
- API tests (in-process axum, no real sockets where possible): missing
  token → 401; wrong token → 401; bad JSON → `Error` message, connection stays
  open; unsupported version → `Error`.
- Config: `0.0.0.0` bind refused with a clear error.
- Screen restore: bytes that draw text at row 5 → reattach frame reproduces it.
- Integration (`#[ignore]`, real PTY): run `sh -c 'echo ready; sleep 2; echo done'`,
  detach, reattach, observe `done`, exit code 0.
- Web: Vitest for the key bar and frame encoder/decoder; one Playwright smoke
  test against a daemon started with a fake backend.

## Done when

The demo works by hand on Linux and macOS (document exact steps in the
report), `just check` passes, and the report is sent. Stop.
