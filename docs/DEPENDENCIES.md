# Approved dependencies

Add to this list (with a reason) before using a new package.
Fewer dependencies = easier to understand, faster builds, fewer surprises.

## In use now

| Crate | Used in | Why |
|---|---|---|
| `serde` | protocol | Serialize/deserialize wire types |
| `serde_json` | protocol | JSON wire format |
| `thiserror` | protocol | Readable error enums |
| `rusqlite` (`bundled`) | store | Local SQLite persistence with a consistent bundled SQLite/FTS5 build |
| `sysinfo` | daemon | Cross-platform CPU, RAM, and process-tree sampling |
| `nvml-wrapper` (optional) | daemon | NVIDIA utilization, VRAM, temperature, and per-process GPU memory |
| `regex` | watcher | Deterministic built-in and declarative output matching |
| `insta` (dev) | watcher | Reviewable Event snapshots from saved real-world logs |
| `sha2`, `hmac` | daemon | Machine-link credential hashes and keyed pairing-code hashes (4.0, approved 2026-10-02) |
| `reqwest` (`rustls-tls`, `json`, no default features) | daemon | Worker-to-control HTTPS for pairing exchange, heartbeat, refresh, and disconnect (4.0, approved 2026-10-02) |
| `web-push-native` 0.5 (`vapid`, no defaults) | daemon | Pure-Rust RFC Web Push encryption and VAPID request construction for the control-owned Push gateway (4.D, approved 2026-10-06) |
| `base64ct` 1.8 | daemon | Strict base64url encoding/decoding for VAPID and browser Push key material (4.D, approved 2026-10-06) |

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
| `ts-rs` | 0 | Generate TypeScript types from protocol |
| `toml` | 1 | Parse daemon TOML for daemon startup and installed CLI address discovery |
| `tokio-tungstenite` | 1/4.A | WebSocket client transport for `ta` and control-to-worker terminal routing |
| `futures-util` | 1/1c.1 | Async WebSocket handling for the CLI and bounded file streaming for the daemon package service |
| `tower` (dev) | 1 | Exercise Axum routes in-process without network sockets |
| `argon2` | 1.1 | Hash and verify the single owner's password with Argon2id |

## Web (Milestone 1+)
`vite`, `typescript`, `react`, `@xterm/xterm`, `@xterm/addon-fit`, `@xterm/addon-webgl`,
`@tanstack/react-query`, `vitest`, Testing Library, Playwright, `react-router-dom`
(client-side application routes), `@fontsource/archivo` and
`@fontsource/jetbrains-mono` (self-host the approved UI fonts)

## Release automation (GitHub Actions)
Build-time only; nothing here ships inside Sentinal. Each action is pinned to a
full commit SHA in `.github/workflows/release.yml`, and
`scripts/tests/release-workflow-test.sh` refuses an unpinned one.

| Action | Why |
|---|---|
| `actions/checkout` | Check out the tagged commit, without persisting credentials |
| `actions/setup-node` | Node 22 for the production PWA build (pnpm through corepack) |
| `actions/attest-build-provenance` | Sigstore-signed build provenance per artifact, verified by the control before publishing |
| `actions/upload-artifact`, `actions/download-artifact` | Move per-target artifacts to the release job |

The runners also use `musl-tools` (Ubuntu apt) to link the Linux musl targets.

**Release signatures** use OpenSSH's `ssh-keygen -Y sign/verify`, a system tool
(OpenSSH 8.2 or newer, preinstalled on macOS and Ubuntu); no crate is added.
The owner signs locally; CI never holds a signing key.

## Intelligence (V2)
`llama.cpp` (server), `fastapi`, `transformers`, `trl`, `peft`, `datasets`
