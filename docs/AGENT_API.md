# Agent API (Milestone 6.0)

Lets an AI agent (Claude Code, Codex, or your own script) watch your Machines
through Sentinal, safely. The agent gets **its own token**, separate from the
Machine token and your browser Session, and the control enforces its level on
every request.

## What an agent can and cannot do

| Level | Name | May | Status |
|---|---|---|---|
| 0 | Observe | Read Machines, Sessions, Events, metrics; watch changes; wait for a state | Available |
| 1 | Annotate | Also add notes and proposed Events, marked *inferred* | Next |
| 2 | Suggest | Also suggest a command or answer that you accept by hand; notify | Next |
| — | Act | Not offered. The protocol cannot name a level 3 | — |

At every level an agent **cannot** type into a terminal, resize or kill a
Session, run a Recipe, pair Machines, manage agents, or read terminal output.
Terminal output stays off for agents until per-agent, per-Session opt-in
ships (decision 3 in `PLAN_V2_AI.md`).

## 1. Create a token

On the control, sign in and open **Settings → Agents**. Choose a name (it
appears in attribution as "via claude") and a level. The token is shown
**once**; only its SHA-256 is stored (`agent-tokens.json`, owner-only).
Revoke it there at any time; it stops working on the next request.

Only your browser Session can create, list, or revoke agents. The Machine
token and agent tokens are refused.

## 2. Connect an MCP client

```sh
claude mcp add sentinal --env SENTINAL_AGENT_TOKEN=sat_… -- \
  sentinal mcp --control https://sentinal.<tailnet>.ts.net
```

For other clients, run `sentinal mcp --control <origin>` over stdio with the
token in `SENTINAL_AGENT_TOKEN` (or `--token-file`, an owner-only file).

Tools (full schemas in [`agent-tools.json`](agent-tools.json)):

| Tool | Returns |
|---|---|
| `whoami` | The agent's name and level |
| `list_machines` | Machines, connection status, running and needs-you counts |
| `list_sessions` | Sessions; filter by Machine (ID or name), state, or `needs_you` |
| `get_session` | One Session with its Events and latest resource use |
| `watch` | Changes after a revision, waiting up to 30 s |
| `wait_for` | Blocks until a Session reaches a state (up to 600 s) |

Example: *"Tell me when the training on pop-os finishes."* The agent calls
`list_sessions {"machine": "pop-os"}`, then `wait_for {"machine": "pop-os",
"session_id": 7, "until": ["COMPLETED", "FAILED"]}`.

## 3. HTTP

Every request sends `Authorization: Bearer sat_…`. Errors are
`{"code": …, "message": …}` with 401 (no or revoked token) or 403 (level too low).

| Route | Body |
|---|---|
| `GET /api/v1/agent/whoami` | `AgentIdentity` |
| `GET /api/v1/agent/snapshot` | `ControlProjectionSnapshot` |
| `GET /api/v1/agent/updates?after=N&wait_ms=M` | `AgentUpdates`: `PROJECTION_DELTA` messages in order (the last `to_revision` is the next `after`), or `SNAPSHOT_REQUIRED` |

Owner routes (browser Session only): `GET /api/v1/agents`,
`POST /api/v1/agents {"name", "level"}` → `CreatedAgent` (the only time the
token is shown), `DELETE /api/v1/agents/{id}`, and
`GET /api/v1/agents/{id}/activity` → `AgentActivity`: the agent's latest
100 calls (tool and outcome, never arguments), newest first, with its
attribution "via claude (level 0)". Settings → Agents shows it under
**Activity**. It is kept in memory and starts empty when the control
restarts; the audit log below is the durable record.

## Audit

`pairing-audit.jsonl` on the control records `AGENT_CREATED`,
`AGENT_REVOKED`, and `AGENT_DENIED` (a request above the agent's level, with
"via name (level N)" in its reason), with the agent ID and never the token.
