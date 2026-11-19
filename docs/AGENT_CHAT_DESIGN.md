# Agent Sessions as a chat: design proposal

Status: **approved 2026-11-19** ("start these changes"), with each decision as
recommended below. **Built:** AC1, AC2 on desktop, AC3 and part of AC4 (see
"Built so far"). Not built: the phone Chat view, and tool counts on the
Session's agent badge.

## The idea in one paragraph

A coding agent's Session is a conversation, but Sentinal shows it as a
terminal: hard to read on a phone, and everything Sentinal knows about the
agent is guessed from the screen. Claude Code already writes each
conversation to a structured file on the Machine. Sentinal reads that file
and shows the Session as a chat: your messages, the agent's replies, and each
command or edit as a card you can open. The terminal stays one tap away and
stays the truth; the chat is another view of the same Session. Typing in the
chat's message box types into the same terminal.

## Why, from using it (2026-11-19)

A hands-on run with Claude Code 2.1.328 (`docs/TASKS.md`, section Q) showed
what reading the screen costs:

- Tool calls were counted as 0 until a new screen shape was added (Q5), and
  quick ones can still be missed.
- The trust menu's question was read as the link above it (Q6).
- Each new agent version can change what the screen looks like.

The file has none of these problems. Read from the test run's file (record
types only, no content), it holds:

| Record | What it carries |
|---|---|
| `user` | your message, or a tool's result (`tool_result` block) |
| `assistant` | the reply (`text`), a tool call (`tool_use`: name and input), its reasoning (`thinking`) |
| every record | `uuid`, `parentUuid`, `timestamp`, `sessionId`, `cwd`, `isSidechain` |
| others | `attachment`, `api-request*`, `file-history-snapshot`, `mode`, … |

It also carries `accountUuid` and `organizationUuid`, which Sentinal must
never pass on (see Privacy).

The format is Claude Code's own and is not documented as stable: the test
file had more than sixty top-level fields. The reader therefore uses only the
few fields above and treats everything else as unknown.

## What the owner sees

- **Session page, agent Sessions:** a `Chat | Terminal` switch. Chat shows:
  - your messages on one side and the agent's replies on the other;
  - each tool call as a one-line card ("Ran `touch one.txt`", "Edited
    `src/api.rs`") that opens to show its input and a short preview of the
    result;
  - an open question as the same card as today (`WAITING_FOR_INPUT`), with
    the same answer buttons;
  - "Agent finished · 2 tool calls" between turns.
- **Message box** at the bottom: sends your text, then Enter, into the
  terminal (stage AC3).
- **When there is no file** (an agent Sentinal cannot read yet, or Claude
  Code without the hook below): no switch, just the terminal, as today.
- **Default view:** see decision 5.

## How it works

### 1. Finding the right file: Claude Code tells Sentinal

Guessing (newest file in the folder, start times) would show the wrong
conversation when two agents run in one folder. Instead, Claude Code says
which file is its own, using its hooks:

1. A `SessionStart` hook runs `sentinal agent-chat link`. Claude Code passes
   the hook a JSON object on stdin that includes `session_id`,
   `transcript_path` and `cwd`.
2. `sentinal agent-chat link` reads `SENTINAL_SESSION_ID` from its
   environment (set by the daemon for every FULL Session,
   `protocol/src/session_env.rs`). Outside a Sentinal Session it exits 0 and
   does nothing, so the hook is harmless everywhere else.
3. It sends a new local-socket request, `AGENT_TRANSCRIPT { session_id, path }`.
   The daemon accepts it with the same check as `AGENT_STATE` (6.4): the
   peer's OS UID is the daemon's, and the peer process descends from that
   Session's process root. The request carries no pid; the OS says who sent it.
4. The path must be: absolute; a regular file, not a symlink; owned by the
   daemon's user; ending in `.jsonl`; and under `~/.claude/projects/`.
   Anything else is refused and audited, like other refused local requests.

Setting up the hook: `sentinal agent-chat setup` shows the exact change to
`~/.claude/settings.json`, and writes it only when the owner confirms
(decision 7). `sentinal agent-chat setup --print` prints it for a person to
add by hand. `sentinal doctor` says whether the hook is installed.

### 2. Reading the file

- The daemon only reads the file, never writes it, and follows it as it
  grows (it is append-only). On restart, the next `SessionStart` hook links
  it again, or the link is kept in memory only (decision 3).
- It keeps only these: user text; assistant text; `tool_use` (tool name, and
  a bounded summary of its input); `tool_result` (ok or error, and a bounded
  preview); timestamps; and `isSidechain` (sub-agent work, folded under the
  call that started it).
- It drops everything else, including the account and organisation IDs,
  `attachment`, the `api-request*` records and file snapshots. Whether
  `thinking` is shown is decision 2.
- **Bounds:**
  - Lines over 1 MiB are skipped.
  - Previews are cut at 4 KB ("more in the terminal").
  - At most the last 500 chat items are held per Session.
  - A line it cannot read is skipped. If too many are skipped, it reports
    "chat unavailable for this Claude Code version", and the switch goes
    back to the terminal.
- **Secrets:** text and previews go through `watcher::redact::redact_secrets`,
  as searchable output already does (decision 4).

### 3. On the wire (additive; old clients ignore it)

Chat items travel the way terminal output does: from the owning Machine to
the owner's browser. They are **never** in the control's dashboard
projection, and **never** in Sentinal's SQLite.

- `CHAT_SNAPSHOT { session_id, items }` when a viewer opens Chat.
- `CHAT_ITEM { session_id, item }` as the file grows.
- `item` is one of:
  - `USER { text }`
  - `AGENT { text }`
  - `TOOL { id, tool, summary }`
  - `TOOL_RESULT { id, ok, preview }`
  - `TURN_FINISHED { tool_uses }`
- Each item also carries `at`. A Session's capabilities gain
  `exposes_chat`: true only for FULL capture with a linked file.

Fixtures go in `crates/protocol/tests/fixtures/`, as for other messages.

### 4. Sending a message

The message box uses the existing input path (`write_input`): it is audited
as metadata only, and refused for read-only captures. It sends the text and
then `\r`. Version 1 is a single line (decision 6): a newline would submit
early in most agents. Paste-safe multi-line input can follow later.

### 5. Questions and answers

There is no change. A question is still read from the screen and answered
with today's buttons. Menus chosen with arrow keys still need T3.4; until
then the card says to answer in the terminal (Q7).

### 6. Tool counts and turns

When a file is linked, tool calls and "turn finished" come from the file.
They replace the screen guesses for that Session, which fixes the rest of Q5.

## Other agents

Each one needs its own reader, built only after capturing a real file from
that agent in `just dev`. Found on this Mac, with formats not yet read:

| Agent | Where its conversations seem to be |
|---|---|
| Antigravity (`agy`) | `~/.gemini/antigravity-cli/conversations`, `history.jsonl` |
| Gemini CLI | `~/.gemini/history` |
| Mistral Vibe | `~/.vibe/logs/session` |
| aider | `.aider.chat.history.md` in the project (by its docs; not checked) |
| Codex | not installed here |

How each one says which file is its own depends on that agent's hooks. One
without hooks stays terminal-only, rather than being guessed.

## Privacy and security

- **The same sensitivity as terminal output.** It holds code, command output
  and possibly secrets. So it is shown only where terminal output may be
  shown, and goes to the owner's browser only.
- **Nothing new is stored.** Sentinal keeps no copy, and the agent's own file
  stays the record. Deleting it removes the chat.
- **Account and organisation IDs** are dropped when the file is read, and a
  privacy test checks they reach no message, log or store.
- **The daemon reads one file per Session.** That file is named by a process
  inside that Session, under the path rules in §1. Sentinal never lists or
  reads other conversations.
- **Changing the owner's Claude Code settings** happens only through
  `sentinal agent-chat setup`, after a preview and a yes.

## Tests

- Reader:
  - fixtures from a real, scrubbed Claude Code 2.1.328 file;
  - unknown fields and record types are skipped;
  - oversized lines are skipped;
  - "chat unavailable" after too many unreadable lines.
- Link:
  - a peer outside the Session is refused;
  - each path rule;
  - outside a Sentinal Session, `agent-chat link` exits 0 and sends nothing.
- Privacy: no `accountUuid` or `organizationUuid` in any wire message, log
  or SQLite row (the sweep `docs/TESTING.md` already describes).
- Web:
  - the chat switch appears only with `exposes_chat`;
  - the message box sends exactly the text and `\r`;
  - tool cards open and close.
- By hand: one real Claude Code run in `just dev`, written up like section Q.

## Build order (one change each, after approval)

- **AC1:**
  - `agent-chat link`, `AGENT_TRANSCRIPT` and the reader in the daemon;
  - `sentinal status <id>` prints the last chat items.
  - No web change.
- **AC2:** read-only Chat view on the Session page (desktop and phone).
- **AC3:** the message box.
- **AC4:** tool counts and turns from the file.
- **AC5:** the next agent, once a real file has been captured.

## Built so far (2026-11-19)

- `protocol/src/agent_chat.rs`: chat entries; `GET_CHAT`, `CHAT`, and the
  local `AGENT_TRANSCRIPT`, with fixtures.
- `watcher/src/agent_transcript.rs`: the reader. Besides chat entries it
  gives progress, which becomes an agent report (6.5), so every surface that
  shows agent progress shows it:
  - the task: the plan item in progress, else what the owner last asked;
  - step N of M, from `TaskCreate` / `TaskUpdate`, or from `TodoWrite`;
  - the activity, e.g. `Running cargo test`;
  - working while a turn runs, done at `end_turn`;
  - time on task, and Sentinal's pace estimate, as before.
- `daemon/src/session_manager/agent_chat.rs`: link (the same in-Session
  check as `AGENT_STATE`), path rules, a read-only follower built on
  `log_tail`, and 500 entries in memory.
- `sentinal agent-chat link | setup [--print]`.
- Web: the `Chat | Terminal` switch on the desktop Session page, the chat
  view, and the message box.

Found while testing with real Claude Code 2.1.328:

- The conversation file appears with the first message, not at
  `SessionStart`, so the link checks the folder and the file is opened when
  it appears.
- The message box sends the text, then Enter 150 ms later. Sent together,
  Claude Code takes them as a paste and does not submit.
- Chat travels only on a Session's attach, which is gated like terminal
  output; the desktop page keeps the terminal attached under Chat. The phone
  page has no attach yet, so its Chat view is still to come.
- Claude Code loads its task tools only on request, so many turns have no
  plan. Those show the task and the live activity, without step N of M.
- In a dev setup, the shell profile can put an older installed `sentinal`
  first on `PATH`. The hook then fails ("unrecognized subcommand").

## Decisions for the owner

Accepted 2026-11-19, as recommended.

1. **Linking.** Approve linking only through the agent's hook (§1), with no
   guessing from folders or times.
2. **Reasoning.** Show the agent's `thinking` blocks? Recommendation: no.
   They are long and are not the conversation.
3. **Storage.** Approve storing nothing: the chat is read from the agent's
   file, and the link to that file is kept in memory only.
4. **Previews.** Approve 4 KB tool previews, with secrets redacted as in
   searchable output.
5. **Default view.** Chat or Terminal for agent Sessions? Recommendation:
   Chat on phones, Terminal on desktop, and remember the owner's choice.
6. **Message box.** Approve a single line in version 1.
7. **Setup.** Approve `sentinal agent-chat setup` editing
   `~/.claude/settings.json` after a preview and a yes.

## Open questions

- Does the file record approval answers? A field named `permissionDecision`
  exists, but it was not read. If it does, a chat could show "you allowed
  `touch one.txt`".
- How sub-agent (`isSidechain`) work should look: folded under its call, or
  hidden.
- Whether `claude -p --output-format stream-json` (Sentinal starting the
  agent itself, without a terminal) is worth a separate "chat-only Session"
  later. It would be a different product shape, so it is not part of this
  proposal.
