# Milestone 4.A progress — Multi-Machine aggregation and routing

Status: **implementation and isolated acceptance complete on 2026-10-04;
two-real-Tailnet-Machine acceptance pending.** Milestone 4.B is in `milestone-4b.md`.

## Architecture delivered

- The browser authenticates only to the control origin. It opens one logical
  `MachineConnection` per active registry Machine through
  `/api/v1/machines/{machine_id}/ws`; worker endpoints and credentials remain
  inside the control daemon.
- Before proxying, the control uses the selected Machine's stored credential
  to call `/api/v1/link/identity` and requires the returned immutable Machine
  ID to match. Both sides reject the credential at its expiry. WebSocket text,
  binary input, resize, Ctrl-C, kill, history, search, Events, Metrics, and
  terminal output therefore use the selected owner only.
- Daemon-local protocol IDs remain unchanged. The web layer represents them
  as `<machine-id>:<numeric-id>`, so Session, Command, Event, and Metric ID 1
  on different Machines cannot collide.
- Registry and Machine queries use a concurrency limit of three. Each
  connection has independent deterministic exponential retry state. Failed
  Machines keep their last complete Session snapshot and status/last-seen
  metadata while successful snapshots replace atomically.
- Event IDs are deduplicated per Machine across reconnects. Browser resume
  resets stale retry delay once and uses the same bounded query fan-out.
- Rename, reconnect, disconnect, and revoke remain Machine-scoped. The UI
  names the Machine in destructive confirmation text and reports Online,
  Offline, Auth expired, Disconnected, and Revoked separately.

No wire-protocol type changed. The only dependency change makes the already
approved workspace `tokio-tungstenite` package a direct daemon dependency for
the control-to-worker WebSocket proxy.

## Routing and credential-isolation evidence

- `crates/daemon/tests/control.rs` pairs two workers with distinct control
  credentials, resolves each routing target by immutable Machine ID, revokes
  one, and proves the other still resolves.
- `web/src/data/api/index.test.ts` gives the control and worker the same local
  Session ID `1`. Input, kill, search, terminal resolution, and replay are
  asserted on the selected Machine connection; the other connection receives
  no frame or request. Every browser socket has the control origin and contains
  only the owner Session token, never either worker credential.
- The same adapter suite makes one worker unavailable, verifies its cached
  snapshot remains, controls the healthy Machine, reconnects the worker, and
  accepts a repeated Event ID only once. Revoking one worker preserves the
  other connection.
- `web/e2e/enrollment.spec.ts` runs a real control daemon plus two isolated
  worker daemons. Both workers create daemon-local Session `1`; the PWA sees
  and attaches both through Machine-scoped control-origin sockets. Worker A
  disconnects while Worker B remains controllable, then reconnects without a
  duplicate Session or Event view. Captured browser WebSocket URLs all use
  port `4173`, and browser storage contains only `sentinal.sessionToken` and
  the theme.

## Automated acceptance record

Host used for the local run:

```text
macOS 27.0 (build 26A428), arm64
Darwin 27.0.0, RELEASE_ARM64_T8132
rustc/cargo 1.97.1
Node 22.23.1
pnpm 10.17.1
```

Commands and results:

```console
$ cargo test -p daemon --test control
22 passed

$ pnpm -C web check
25 test files passed; 115 tests passed; TypeScript passed

$ pnpm -C web test:e2e
5 passed in 31.7s
# control 127.0.0.1:7421; workers 127.0.0.1:7431 and :7432

$ just check
# cargo fmt, clippy -D warnings, workspace tests, generated bindings,
# 115 web tests, and repository shell checks passed
```

Fast tests use no real network and no sleep over 100 ms. Playwright uses only
three isolated loopback daemons with fresh credentials and data directories.

## Real two-Machine acceptance

**NOT RUN on 2026-10-04.** `tailscale status --json` and `tailscale version`
both failed on this host with `The Tailscale CLI failed to start: Failed to
load preferences.` The earlier 4.0 report records a macOS 26.6.2 MacBook and
a pop-os Linux worker, but it does not prove 4.A Session routing. Their old
enrollment run is not counted as this acceptance.

Run and append the literal output below once the control and two workers are
available. Do not put tokens or pairing codes in this report.

```console
# Control Machine
sw_vers || cat /etc/os-release
uname -a
tailscale version
tailscale ip -4
sentinal connection status --json

# Worker A and Worker B (run on each)
sw_vers || cat /etc/os-release
uname -a
tailscale version
tailscale ip -4
sentinal connection status --json
sentinal watch -t "4.A routing $(hostname)" -- sh -lc \
  'printf "machine=%s ready\\n" "$(hostname)"; exec ${SHELL:-/bin/sh}'
```

Then, from the phone PWA at the control origin:

1. Record both Machine names, immutable IDs, OS versions, Tailscale IPv4
   addresses, endpoints, and the tested commit.
2. Confirm both named Sessions appear under their owning Machines even when
   each daemon-local Session number is the same.
3. Attach to A, enter `printf 'A-only\\n'`, resize, and send Ctrl-C. Confirm B
   receives no input and its terminal does not change.
4. Attach to B, enter `printf 'B-only\\n'`, then search history for `B-only`.
   Confirm the one-Machine filter returns B only and all-Machine search reports
   both available Machines.
5. On A run `sentinal disconnect`. Confirm A retains its last snapshot with
   `DISCONNECTED` and last-seen time while B remains attachable and accepts
   input. Re-enroll A with a fresh Add Machine code and confirm its Session and
   Event rows are not duplicated.
6. Revoke A in Settings. Confirm B stays Online, its active terminal stays
   usable, and the confirmation names A. Inspect browser storage and captured
   requests for absence of pairing codes, Machine tokens, worker credentials,
   direct worker origins, and B frames on A routes.

The final roadmap box remains open until this exact run, including the two OS
versions and command output, is appended here.
