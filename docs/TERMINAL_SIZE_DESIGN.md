# One size per Session: wire proposal (T3)

Status: **approved 2026-11-22** with the four recommended decisions, and
**built** (T3.1–T3.6; `docs/TASKS.md` section T). One difference from the
proposal: the CLI cannot crop rows it has no room for (it keeps no screen of
its own); it cuts long lines at its edge and shows the status line. Source: `prompts/13-terminal-upgrade-package.md` §T3 and owner decision
1 there ("one size per Session, set by the latest active viewer"). This
refines the prompt's sketch against the code as it is after T2 (`050572e`).

## The problem in one paragraph

Every viewer sends `RESIZE` and the PTY takes whichever came last. Two
viewers of different sizes (laptop and phone, or the web and `sentinal
watch`) therefore fight: the program redraws for the last one, and every
other viewer draws those bytes at the wrong size, wrapping and smearing.
After T2 the daemon has a real screen at the Session's size, so it can own
the size and hand every viewer a whole screen at that size.

## What the code does today (read 2026-11-22)

- `RESIZE` resizes the PTY and the screen at once, from any connection
  (`api.rs`, `SessionManager::resize`). Bounds: anything above 0.
- The web sends `RESIZE` before `ATTACH` and again after the snapshot,
  and on every fit (`web/src/terminal/useRealTerminalSession.ts`). Peek
  cards never resize and draw at a fixed 120×24 (`terminal/peek.ts`).
- The CLI sends `RESIZE` before `ATTACH` and on `SIGWINCH`
  (`crates/cli/src/client.rs`).
- The control relays each browser socket one-for-one to its worker
  (`control/relay.rs`) and passes frames through unchanged, so new message
  types need no control change. On the worker every viewer behind the
  control has the control's address.
- **Compatibility finding:** the CLI decodes every daemon message with `?`
  (`client.rs:179, 342, 395`). An installed CLI that receives a message type
  it does not know stops with an error. Old cached web pages log and ignore.
  So new daemon messages go only to viewers that said they understand them.

## Wire changes

All additive; `PROTOCOL_VERSION` stays 1.

### `ATTACH` gains three optional fields

```json
{"type":"ATTACH","session_id":7,"history":true,"cols":132,"rows":40,"mode":"INTERACTIVE"}
{"type":"ATTACH","session_id":7,"cols":60,"rows":18,"mode":"OBSERVE"}
```

- `cols`, `rows`: this viewer's box, in cells. Together or not at all.
- `mode`: `INTERACTIVE` (default) or `OBSERVE`. An observer never sets the
  size and its input is refused (peek cards, read-only pages).
- **A viewer that sends `cols` and `rows` is a "sized viewer":** it gets
  `SESSION_SIZE` messages. A viewer that sends neither is an old client and
  gets exactly what it gets today.
- Attaching does not make a viewer active, so opening a Session on a phone
  never resizes it under the laptop. Exception: when the Session has no
  active viewer yet, the first interactive viewer becomes active.

### New client message `VIEWER_STATE`

```json
{"type":"VIEWER_STATE","session_id":7,"cols":132,"rows":40,"active":true}
```

Sent when the viewer's box changes, or when it gains (`active: true`) or
loses (`active: false`) focus. Only for a Session this connection is
attached to; otherwise an error.

### New daemon message `SESSION_SIZE`

```json
{"type":"SESSION_SIZE","session_id":7,"cols":132,"rows":40,"owner":true}
```

Sent to every sized viewer of the Session: once on attach (before the
snapshot frame), and on every change of size, each time followed at once by
a binary frame with a terminal reset and a fresh snapshot at the new size.
`owner` is true for the viewer whose report set the size. Viewers draw at
this size; they do not resize anything themselves.

### `RESIZE` from an old client

Treated as `VIEWER_STATE` with `active: true`, so old CLIs and cached pages
keep working exactly as today (they set the size when they report it).

### New client message `ANSWER_PROMPT` (T3.4)

```json
{"type":"ANSWER_PROMPT","session_id":7,"event_id":812,"option":2}
```

Answers a menu by its place in the list, not by keys. The daemon:

1. Refuses unless the Session is `WAITING` and its current
   `WAITING_FOR_INPUT` Event is `event_id` ("the question changed").
2. Reads the menu from the current screen (the watcher's screen-prompt
   reader on `Screen::rows`) and refuses unless it has the same options as
   the Event ("the menu changed").
3. Sends arrow keys from the marked row to `option`, then Enter, through the
   normal input path (audited, capture rules apply).

The answer is `SESSION_UPDATED`, or `ERROR` with the reason. It replaces the
Vibe-only "answer this in the terminal" logic in `web/src/data/actions.ts`.
Digit and `y`/`n` answers keep using plain input: the program's own keys
already name the answer.

### Fixtures

New entries in `crates/protocol/tests/fixtures/client_requests.json`:
`ATTACH` with size and mode (interactive and observe), `VIEWER_STATE`,
`ANSWER_PROMPT`. In `daemon_messages.json`: `SESSION_SIZE` with
`owner: true` and `false`. Generated TypeScript via `just gen-ts`. Release
notes in `docs/TASKS.md`.

## Size policy (daemon only)

Per Session, the daemon keeps each viewer's last report: box, mode, and when
it was last active. A viewer is one attachment on one connection.

- **Active viewer:** the interactive viewer that most recently typed (any
  input frame from that connection to this Session) or sent
  `active: true`. Input from recipes or the agent API does not count.
- **Settle time:** the Session takes the active viewer's size after 150 ms
  with no newer report, so dragging a window edge resizes once.
- **Active viewer leaves:** the size stays until another viewer becomes
  active. Nothing shrinks or grows on its own.
- **Observers** never become active and never change the size.
- **Bounds:** 20–500 columns, 5–200 rows. A report outside them is clamped
  for the Session; the viewer still gets `SESSION_SIZE` with the real size.
- **Audit:** every accepted size change is appended to the audit log with
  the client address, like input: `{"timestamp", "client_addr",
  "session_id", "resize": {"cols", "rows"}}` (the record gains an optional
  `resize`; `byte_count` is 0).
- **Tests** (T3.2) drive the policy with an injected clock: two viewers
  alternating, a viewer leaving, an observer, an old client's `RESIZE`,
  the settle time, clamping.

How a resize reaches viewers: the daemon resizes the PTY and the screen,
then publishes the new size on the Session's structured channel with the
owner's viewer id. Each sized viewer's forwarder (`api/attach.rs`) already
listens there: it sends `SESSION_SIZE` with `owner` set for itself, then a
reset and a snapshot taken through `ScreenSource::resync`, switching to the
receiver that snapshot returns. That is the same exactly-once split T2 gives
an attach, so no byte is lost or doubled across a resize.

## Clients

- **Web (T3.3):** attaches with its box and `INTERACTIVE`; sends
  `VIEWER_STATE` on fit (debounced as today) and on focus/blur, and treats
  typing as becoming active. It sets its terminal to the Session size. When
  it is not the owner and the Session is wider than its box, it scales the
  font down to fit the width, not below 9 px, then pans; a one-line bar
  reads "Sized for another device · Use this size", and a tap sends
  `active: true`. Peek cards attach as `OBSERVE` and render at the Session
  size, scaled to the card.
- **CLI (T3.5):** attaches with its window size; `SIGWINCH` sends
  `VIEWER_STATE`; typing makes it active. When it is not the owner it crops
  or pads to its window and shows one status line, "Sized for another
  device (132×40) · type to take over".
- **Browser test (T3.6):** two pages of different sizes on one Session,
  type in each in turn; both screens stay identical cell for cell.

## Decisions for the owner

1. **Attaching does not take the size** (only typing or focus does), except
   for the first viewer. Recommended: yes, so looking from a phone never
   disturbs the laptop. Alternative: the newest viewer always takes it.
2. **Focus counts as active**, not only typing. Recommended: yes for the web
   (clicking into a terminal means "I'm here"), and the CLI becomes active
   only by typing, since a terminal window has no reliable focus signal.
3. **`ANSWER_PROMPT` by option number with the Event id.** Recommended:
   yes. It is also what the phone and the agent API need for arrow menus.
4. **Smallest font when scaling to fit: 9 px**, then pan. Recommended: 9 px.

## Not in this stage

The keeper (T5), `sentinal watch` pick-up with history (T4), and anything a
multiplexer would need.
