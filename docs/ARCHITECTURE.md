# Architecture

## Shape

One Rust binary per machine (`sentinald`, currently built as `tad`), a
TypeScript web app, the `sentinal` CLI (currently `ta`), and an
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
├── intelligence/        Python agent + training       (V2)
└── recipes/             built-in TOML recipes         (Milestone 4)
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
