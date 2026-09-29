# Milestone 1b.E progress — Finished Session lifecycle and output replay

Status: complete 2026-09-29; awaiting human review before Milestone 1c.

## Tasks and proof

- [x] Centralize terminal-state gates for input, resize, kill, and future
  signal operations — `crates/daemon/tests/session_manager.rs` and
  `crates/daemon/src/api/capture_tests.rs`.
- [x] Reconcile a closed PTY write race without exposing an OS `EIO` or
  auditing rejected bytes — `crates/daemon/tests/session_manager.rs`.
- [x] Replay persisted compressed output with bounded work and memory —
  `crates/daemon/tests/output_replay.rs`.
- [x] Keep finished Sessions visibly read-only while live detached Sessions
  stay interactive — `web/src/terminal/lifecycle.test.ts` and
  `web/src/routes/app/SessionDetailPage.test.tsx`.
- [x] Continue from safe metadata into one distinct UI/FULL Session without
  replaying an old Command — `web/src/routes/app/SessionDetailPage.test.tsx`
  and `web/src/data/api/index.test.ts`.
- [x] Prove the complete browser flow — Playwright finished-Session lifecycle
  smoke test.

## Additive replay HTTP contract

`GET /api/v1/sessions/{session_id}/output?offset={offset}&limit={limit}`
returns a page of decompressed terminal bytes. The route uses the existing
Bearer/browser-session authentication middleware. `session_id` must be a
decimal protocol Session ID; it is never accepted as a filesystem path.

- Response content type: `application/octet-stream`. The bytes retain ANSI
  and other terminal control sequences and are only suitable for a terminal
  emulator or a binary file.
- `offset` is an optional zero-based offset in the decompressed byte stream
  and defaults to zero. `limit` defaults to 262,144 bytes, must be at least
  one, and is capped at 1,048,576 bytes.
- A request may scan at most 67,108,864 decompressed bytes (`offset + limit`).
  Larger requests receive `400 OUTPUT_RANGE_TOO_LARGE`. This bounds memory,
  decompression work, and response size even when the underlying log is very
  large.
- Successful responses include `X-Sentinal-Output-Offset`,
  `X-Sentinal-Output-Next-Offset`, and `X-Sentinal-Output-Complete` headers.
  Clients page by using the next offset until complete is `true`.
- If a final zstd frame is corrupt or truncated, bytes produced before the
  decoder error are returned and `X-Sentinal-Output-Warning` is
  `TRUNCATED_ZSTD`. The daemon never crashes and the UI displays the warning.
- An unknown Session is `404 UNKNOWN_SESSION`; a known Session with no log is
  `404 OUTPUT_NOT_FOUND`; reduced capture is `403 OUTPUT_UNAVAILABLE`; bad
  ranges are `400`. Errors are bounded JSON objects with stable codes and
  messages. Authentication failure remains `401`.
- The implementation resolves only
  `<configured data_dir>/sessions/<numeric-id>.log.zst`. It does not use cwd,
  adopted log paths, titles, or any Session-supplied path, and therefore
  cannot traverse directories or follow a caller-selected path.
- ANSI preview and export use this one paged resource. There is no raw
  compressed-download route in 1b.E: it would add a second retention surface
  without helping the UI. Operators can concatenate pages into a binary ANSI
  export. Plain-text conversion is intentionally omitted because stripping
  terminal control sequences correctly is a separate policy decision.

This resource does not modify protocol JSON or fixtures and keeps
`PROTOCOL_VERSION = 1`. It uses the existing `zstd`, Axum, and Tokio
dependencies.

## Open questions

- None blocking. Remote replay is intentionally limited to the first 64 MiB
  of decompressed output in this slice; an indexed log format is the future
  path for efficient arbitrary offsets in larger recordings.

## Verification (2026-09-29, macOS)

- `just check`: passed format, all-target/all-feature Clippy with warnings
  denied, 361 fast Rust tests (including 21 protocol contract tests), 65
  Vitest tests, TypeScript compilation, generated-type verification, and the
  network launcher test. Final line: `run-network test: passed`.
- `pnpm -C web exec playwright test --reporter=line`: 3 passed, including
  finish → authenticated replay → no binary input → Continue to a distinct
  Session ID.
- Replay behavior tests cover valid multi-frame and empty logs, missing and
  unknown Sessions, unauthorized and traversal-shaped requests, the 1 MiB /
  64 MiB bounds, a truncated final frame, reduced capture, and a persisted
  Session with no in-memory PTY after restart.
- Lifecycle tests cover all three terminal states, the closed-device race,
  live detach/reattach, retained in-memory snapshot attach, fake PTY operation
  counters, and absence of rejected bytes from audit, Store, and output log.

## Try replay/export

```sh
TOKEN='paste-machine-or-browser-token'
curl --fail-with-body \
  -H "Authorization: Bearer $TOKEN" \
  -D session-12.headers \
  -o session-12.part-0.ansi \
  'http://127.0.0.1:7420/api/v1/sessions/12/output?offset=0&limit=1048576'
```

Read `X-Sentinal-Output-Next-Offset` from the saved headers and request the
next page when `X-Sentinal-Output-Complete` is `false`. The `.ansi` file is
untrusted terminal data; display it through a terminal emulator and never
source or execute it.
