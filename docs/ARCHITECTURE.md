# Architecture

## Shape

One Rust daemon per machine (`sentinald`, with `tad` as a compatibility
alias), a TypeScript web app, the `sentinal` CLI (`ta` compatibility alias), and an
optional Python **intelligence** service. They all speak one protocol,
defined once in `crates/protocol`.

```
 Phone / Desktop browser       sentinal CLI        ordinary shells/processes
          │                         │                         │
          │             WebSocket + local socket              │
          └─────────────────────────┼─────────────────────────┘
                                    │
   ┌────────────────────── sentinald (per machine) ────────────────┐
   │ ingress → sessions (PTY/external/adopted) → metrics → store   │
   │                  │                         │                   │
   │            command tracking             watcher → Events      │
   └───────────────────────────────────────────────────────────────┘
                                   ▲
          intelligence (optional) ─┘  same API as the UI, no special access
```

## Repository layout

Crates are added **only when their milestone starts** (see `ROADMAP.md`).

```
sentinal/
├── AGENTS.md            rules for humans and AI agents
├── Cargo.toml           Rust workspace
├── justfile             common commands
├── docs/                architecture, roadmap, testing, dependencies
├── crates/
│   ├── protocol/        shared wire types            (Milestone 0)   ✅
│   ├── daemon/          PTY sessions, API             (Milestone 1)
│   ├── store/           SQLite                        (Milestone 2)
│   ├── watcher/         output → Events               (Milestone 3)
│   └── cli/             command-line client           (Milestone 1)
├── shell/               zsh/bash/fish hooks (OSC 133) (Milestone 2)
├── scripts/             development launchers and offline tests
├── web/                 Vite + xterm.js PWA           (Milestone 1)
├── eval/                deterministic model-evaluation foundation
├── recipes/             built-in TOML recipes         (Milestone 4)
├── design_handoff_sentinal/ source design references
└── marketing/           reproducible product-film source
```

## Key decisions

| Decision | Choice | Why |
|---|---|---|
| Daemon language | Rust | Always-on, small, reliable |
| Session persistence | Own PTY (`portable-pty`) + VT parser | Structured state is the product |
| Capture ingress | Watch, shell hook, auto-wrap, or read-only adopt | Work can enter Sentinal without opening Sentinal first |
| Local transport | Private per-user Unix socket, mode `0600`, peer UID checked | Shell hooks stay fast and never expose control publicly |
| Command boundaries | Shell hooks emitting OSC 133 | Exact, no output guessing |
| Storage | SQLite (WAL, FTS5) | Local-first, zero ops |
| Wire format | JSON with `version` field; binary frames for terminal bytes | Easy to debug |
| Timestamps | Unix milliseconds (`i64`) | Same in Rust, TS, Python |
| Networking | Tailscale only | No public exposure |
| AI | Separate process, suggest-only | Lower layers work without it |

## Deterministic Event engine

Each live Session owns a pure watcher engine. PTY or adopted-log bytes first
pass through bounded ANSI-free line framing; structured Command boundaries,
process-tree samples, and Metrics enter as typed observations. The always-on
Generic adapter and at most one Command-matched adapter produce `EventKind`
values. `sentinald` assigns Event IDs, persists them in SQLite, and broadcasts
them to clients. The web rebuilds its bounded timeline with `GET_EVENTS` and
never parses terminal text itself.

Current adapters cover training, pytest, vLLM/llama.cpp, Cargo, and validated
TOML rules in `~/.config/terminal-agent/adapters/`. Health is deterministic
derived state with bounded evidence, stored on the Session and delivered by
the existing `SESSION_UPDATED` message. No watcher path uses a model.

## Installation and Machine enrollment

One owner-selected Machine may run the local control role in addition to its
normal `sentinald` responsibilities. It serves verified worker packages and is
the approved authenticated origin for the PWA; the source-tree path still uses
a separate Vite development server, and production PWA delivery remains a V1
gate. This is local infrastructure, not a required cloud service. The browser
authenticates to the control origin. The control daemon maintains one
independently credentialed connection per enrolled worker and routes
terminal/data operations only to the Machine that owns the selected Session.

Installation and authority are separate boundaries:

```text
Tailnet HTTPS package service
          │ verified release
          ▼
 installed worker (UNPAIRED, stable Machine ID)
          │ sentinal connect + one-time code
          ▼
 control Machine registry (PAIRED, unique revocable credential)
```

- The package URL contains no Machine token, owner password, pairing code, or
  per-Machine credential.
- The immutable Machine ID identifies one installation across rename,
  reconnect, update, and control-URL changes. The display name is editable.
- `sentinal connect` receives its one-time pairing code through a hidden TTY
  prompt or bounded stdin, never argv, a URL, environment, or shell history.
- Pairing exchanges the code for a credential unique to that Machine. The
  Machine token stays local and remains the daemon's local CLI/bootstrap
  boundary.
- Before proxying terminal or output traffic, the control authenticates the
  worker's `/api/v1/link/identity` response and pins its immutable Machine ID.
  Worker endpoints and credentials never enter browser responses or storage.
- Removing or revoking a Machine does not uninstall it or delete its local
  Sessions. Reconnecting is an explicit owner-authorized action.

## Capture boundaries

Every Session declares an origin and a capture level. These fields are
security capabilities, not presentation hints:

| Capture | Daemon owns PTY | Output | Commands | Metrics | Remote input |
|---|---:|---:|---:|---:|---:|
| `FULL` | yes | yes | yes | yes | yes |
| `COMMANDS` | no | no | yes | yes | no |
| `PROCESS_LOG` | no | log only | no | yes | no |
| `PROCESS` | no | no | no | yes | no |

The daemon enforces these capabilities for every client, including the
intelligence service. Reduced-capture Sessions cannot attach, accept terminal
input, resize, or be signalled through an adoption API. See
`CAPTURE_MODES.md` for lifecycle and ingress details.

## Session continuation

A finished PTY is never revived. `CONTINUE_SESSION` creates a new `UI` / `FULL`
Session with a new ID, PID, PTY, compressed output log, and audit boundary. Its
optional `continued_from` field points to the immutable predecessor. SQLite
enforces at most one direct successor, so continuation histories are linear and
idempotent under a double-click or concurrent clients.

The web assembles one bounded history from those persisted Session generations.
Each generation is replayed into a separate xterm instance so historical ANSI
state cannot affect another generation. Only the latest live generation can
receive input or resize requests. Ancestor output remains in its original
per-Session log and is never copied into a descendant PTY or log.

## Native notification flow

Eligible stored Events become closed, privacy-safe notification intents. An
enrolled worker persists an intent in its SQLite outbox and uploads it with its
existing Machine-link credential. The control derives the Machine identity,
accepts each idempotency ID once, and creates one independent delivery per
active owner browser subscription.

Only the control owns the VAPID private key and browser subscriptions. Its
provider scheduler encrypts the bounded payload, re-resolves and rejects unsafe
provider addresses on every attempt, pins approved DNS answers, refuses
redirects, and persists retry state outside Event and PTY paths. Provider mode
is mutually exclusive: `WEB_PUSH`, `NTFY_LEGACY`, or `DISABLED`. Future APNs
and FCM providers may reuse intent, policy, routing, and journal semantics, but
their credentials and device tokens remain provider-specific.
