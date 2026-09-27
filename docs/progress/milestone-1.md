# Milestone 1 plan

## Tasks

- [x] Create the daemon and CLI crates plus the minimal web project; prove the
  workspace remains buildable with crate and web checks.
- [x] Implement config loading, safe bind validation, token creation, constant-time
  authentication, and audit records; prove them in daemon unit tests and
  `crates/daemon/tests/api.rs`.
- [x] Implement `PtyBackend`, real and fake PTYs, `SessionManager`, screen restore,
  bounded scrollback, compressed logs, attach/detach, resize, input, exit, and
  kill; prove them in `crates/daemon/tests/session_manager.rs`.
- [x] Implement the Axum health, machine, sessions, and WebSocket API with binary
  framing; prove request validation and connection recovery in
  `crates/daemon/tests/api.rs`.
- [x] Implement `ta ls`, `new`, `attach`, and `kill` against the WebSocket API;
  prove argument and frame handling with CLI unit tests.
- [x] Implement the React/xterm session list and terminal, resize handling, token
  entry, and mobile key bar; prove framing and keys with Vitest and the main flow
  with one Playwright smoke test.
- [x] Add systemd and launchd user service files, document the manual demo, run all
  Rust/web/integration checks, update the roadmap, and commit the milestone.

## Open questions

- None. The three additional Rust dependencies needed for the CLI and in-process
  API tests were approved by the owner before implementation.

## Verification

- `just check` passes Rust formatting, all-target/all-feature Clippy with warnings
  denied, 134 fast Rust tests, 30 TypeScript generation checks, and 9 Vitest tests.
- `just test-integration` passes the real PTY persistence test. The raw terminal
  integration test also passed inside a controlling pseudo-terminal.
- `pnpm -C web test:e2e` passes the Chromium flow against `tad` with `FakePty`:
  authenticate, create a Session, attach, and render xterm.
- The macOS manual demo created a real zsh Session, ran `python long_job.py`,
  detached after its first progress line, and restored the complete screen through
  `done` in a fresh `ta attach`. Both demo Sessions then transitioned to
  `INTERRUPTED` through `ta kill`.

## Try it locally

Install the web packages once, then start the daemon and UI in separate terminals:

```sh
corepack prepare pnpm@10.17.1 --activate
pnpm -C web install
cargo run -p daemon --bin tad
```

```sh
pnpm -C web dev
```

Open `http://127.0.0.1:5173`, then enter the token from
`~/.config/terminal-agent/token`. For a CLI persistence demo, create a long job and
start a Session:

```sh
printf 'import time\nprint("ready", flush=True)\ntime.sleep(10)\nprint("done", flush=True)\n' > /tmp/long_job.py
SESSION_ID=$(cargo run --quiet -p cli --bin ta -- new --cwd /tmp)
cargo run --quiet -p cli --bin ta -- attach "$SESSION_ID"
```

Inside the attached Session run `python long_job.py`, then press `Ctrl-\` followed
by `d`. Reattach after it finishes:

```sh
cargo run --quiet -p cli --bin ta -- attach "$SESSION_ID"
```

For another Tailscale-connected device, set `bind` in
`~/.config/terminal-agent/daemon.toml` to this machine's Tailscale IP, run
`pnpm -C web dev --host <TAILSCALE_IP>`, and open the shown URL on the device.
