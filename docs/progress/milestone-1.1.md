# Milestone 1.1 plan

## Tasks

- [x] Add the single-user authentication HTTP contract to `crates/protocol`;
  prove every request, response, and failure shape with contract fixtures and
  generated TypeScript checks.
- [x] Implement persistent Argon2id password storage, 24-hour in-memory browser
  sessions, injected-clock expiry, per-IP lockout, logout, and password change;
  prove normal and edge cases in `crates/daemon/tests/auth_audit.rs`.
- [x] Add setup, status, login, session validation, logout, and password-change
  routes while preserving machine-token CLI access; prove authorization and
  recovery behaviour in `crates/daemon/tests/api.rs`.
- [x] Wire the React setup/login/account flows to those endpoints and use the
  resulting session token for WebSockets; prove the adapter with Vitest and the
  browser flow with Playwright.
- [x] Run `just check` and the Playwright smoke test, update the roadmap and this
  checklist, and commit one logical authentication change.

## Open questions

- None. The owner approved one account, bootstrap setup with the existing
  machine token, Argon2id password hashing, 24-hour browser sessions, in-memory
  lockout/session state, and continued machine-token authentication for the CLI.

## Verification

- `just check` passes Rust formatting, all-target/all-feature Clippy with warnings
  denied, 147 fast Rust tests, 38 TypeScript generation checks, and 38 Vitest tests.
- `pnpm -C web test:e2e` passes the Chromium flow against `tad` with `FakePty`:
  bootstrap the owner password, enter the protected app, create a Session, and
  attach xterm.
- The authentication HTTP contract is additive, remains protocol version 1, and
  has a fixture for every new request, response, failure, and status type.

## Try it locally

Start the daemon and API-backed UI in separate terminals:

```sh
cargo run -p daemon --bin tad
```

```sh
VITE_DATA=api pnpm -C web dev
```

Open `http://127.0.0.1:5173`. On the first visit, copy the machine token from
`~/.config/terminal-agent/token`, enter it once on the setup screen, and choose
a password of at least 12 characters. Later visits use only that password. The
Argon2id hash is stored in `~/.config/terminal-agent/auth.json`; the machine
token remains available to the local `ta` CLI.
