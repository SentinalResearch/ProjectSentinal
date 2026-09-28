# Security and privacy

This document separates protections implemented today from Milestone 1b
requirements. A planned guarantee is not considered active until its tests
and roadmap checkbox are complete.

## Current remote boundary

- The daemon accepts only loopback or Tailscale addresses and rejects
  `0.0.0.0`, `::`, and ordinary LAN addresses.
- `/api/v1/health` is public; every other HTTP or WebSocket route requires a
  bearer credential.
- The Machine token is generated from 256 random bits, stored mode `0600`,
  compared without early exit, and retained for CLI/bootstrap access.
- One owner password is stored only as an Argon2id hash. Browser sessions
  expire, logout and password change revoke them, and password attempts are
  rate-limited per client address.
- Every remote terminal input frame appends timestamp, client address,
  Session ID, and byte count to `audit.jsonl`. The input payload is excluded.
- PTY output is retained in compressed Session logs. Terminal output is
  untrusted and is never executed.

## Data Sentinel records

| Data | Stored | Notes |
|---|---:|---|
| Terminal output from FULL Sessions | yes | compressed append-only Session log |
| Exact Command text and cwd | yes | structured SQLite history |
| Command exit and duration | yes | structured SQLite history |
| Machine/Session Metrics | yes | raw retention then minute aggregates |
| Remote input metadata | yes | audit JSONL, without payload bytes |
| Raw keystroke payload as its own log | no | must remain absent |
| Password plaintext | no | only Argon2id hash is stored |
| Explicit adopted log content | planned | PROCESS_LOG only, read-only source |

Commands may appear in PTY output because an interactive shell echoes them.
Hidden prompts such as password entry disable echo and therefore do not
appear in the output log. No component may add a separate keystroke recorder.

## Milestone 1b local boundary (planned)

External shell reporting, auto-wrap, and adoption use a private per-user Unix
socket. The implementation must:

- create the socket with mode `0600` in a user-owned directory;
- reject a peer whose OS-reported UID differs from the daemon UID;
- use bounded, versioned messages and recover from malformed messages;
- remove only a verified stale socket owned by the same user;
- refuse symlink and ownership-confusion paths;
- expose no terminal-input operation on the local reporting protocol;
- keep hook reporting silent and asynchronous when unavailable.

Watch is an interactive CLI action and fails closed: if the daemon cannot
create the FULL Session, it prints a clear error and does not execute the
requested command. Shell hooks and auto-wrap fail open: hooks silently do
nothing, and auto-wrap starts the original shell within 300 ms.

## Capture capability enforcement (planned)

The daemon—not the UI—maps capture levels to allowed operations:

| Operation | FULL | COMMANDS | PROCESS_LOG | PROCESS |
|---|---:|---:|---:|---:|
| Read screen / attach | yes | no | no | no |
| Send terminal input | yes | no | no | no |
| Resize PTY | yes | no | no | no |
| Observe Commands | yes | yes | no | no |
| Observe Metrics | yes | yes | yes | yes |
| Read configured log | n/a | no | yes | no |
| Signal adopted process | no adoption | no | no | no |

Adoption is same-user and read-only. It does not signal, write to, debug,
reparent, or claim an exit code for a process Sentinel did not start.

## Intelligence permissions

The intelligence service is an ordinary authenticated client. Initial levels
are observe, summarize, and suggest (Levels 0–2). It cannot adopt or signal a
process. Terminal input is allowed only for a future explicitly authorized
action on a FULL Session; model output alone never grants that authorization.

Terminal output and tailed logs are untrusted data, not instructions. They
must not be interpolated into a shell command or treated as a policy source.

## Security release checks

- Binding and authentication behavior tests pass.
- Local socket mode and peer UID tests pass on Linux and macOS.
- Wrong-user, malformed, oversized, stale-socket, and symlink cases fail
  safely.
- Every reduced capture level is rejected by attach/input/resize/kill tests.
- Storage inspection finds no raw input payload in SQLite, audit JSONL, or a
  separate file.
- Hook/auto-wrap daemon-down tests prove the documented fail behavior.
