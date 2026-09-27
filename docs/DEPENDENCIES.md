# Approved dependencies

Add to this list (with a reason) before using a new package.
Fewer dependencies = easier to understand, faster builds, fewer surprises.

## In use now

| Crate | Used in | Why |
|---|---|---|
| `serde` | protocol | Serialize/deserialize wire types |
| `serde_json` | protocol | JSON wire format |
| `thiserror` | protocol | Readable error enums |

## Approved for later milestones

| Crate | Milestone | Why |
|---|---|---|
| `tokio` | 1 | Async runtime for the daemon |
| `axum` | 1 | HTTP + WebSocket API |
| `portable-pty` | 1 | Spawn shells in a PTY (Linux + macOS) |
| `vt100` or `alacritty_terminal` | 1 | Keep screen state for reattach |
| `tracing`, `tracing-subscriber` | 1 | Logging |
| `clap` | 1 | CLI argument parsing |
| `zstd` | 1 | Compress scrollback logs |
| `rusqlite` or `sqlx` | 2 | SQLite storage |
| `sysinfo` | 2 | CPU / RAM / processes |
| `nvml-wrapper` | 2 | NVIDIA GPU metrics |
| `regex` | 3 | Watcher adapters |
| `insta` (dev) | 3 | Snapshot tests for watcher output |
| `ts-rs` | 0 | Generate TypeScript types from protocol |
| `toml` | 1 | Parse the required daemon TOML configuration |
| `tokio-tungstenite` | 1 | WebSocket client transport for `ta` |
| `futures-util` | 1 | Async WebSocket stream and sink handling for `ta` |
| `tower` (dev) | 1 | Exercise Axum routes in-process without network sockets |
| `argon2` | 1.1 | Hash and verify the single owner's password with Argon2id |

## Web (Milestone 1+)
`vite`, `typescript`, `react`, `@xterm/xterm`, `@xterm/addon-fit`, `@xterm/addon-webgl`,
`@tanstack/react-query`, `vitest`, Testing Library, Playwright, `react-router-dom`
(client-side application routes), `@fontsource/archivo` and
`@fontsource/jetbrains-mono` (self-host the approved UI fonts)

## Intelligence (V2)
`llama.cpp` (server), `fastapi`, `transformers`, `trl`, `peft`, `datasets`
