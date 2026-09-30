# Milestone 1c progress — Deployable worker and two-host acceptance

Status: complete, including the real two-host acceptance run on 2026-09-30.

## Tasks and proof

- [x] Extract shared Tailscale/address/port/config helpers and add distinct
  combined, agent-only, and control-only launch roles —
  `scripts/tests/run-roles-test.sh`.
- [x] Add `just agent` and `just control WORKER_IP=...`, force real API mode,
  reject unsafe addresses and occupied ports, and clean up child processes —
  `scripts/tests/run-roles-test.sh`.
- [x] Add an offline per-user worker installer for source trees and prebuilt
  release directories, including dry-run, reinstall/upgrade, non-service
  mode, safe source validation, and data-preserving uninstall —
  `scripts/tests/install-worker-test.sh`.
- [x] Render systemd and launchd user services with exact installed paths and
  no credentials, including paths containing spaces —
  `scripts/tests/install-worker-test.sh` snapshots/assertions.
- [x] Prove browser authentication and WebSocket attach through the control
  proxy against the fake daemon — `web/e2e/session-smoke.spec.ts` through the
  control-role Playwright server configuration.
- [x] Document setup, service operations, backup/recovery, inspection, current
  boundaries, and the manual two-host checklist — `docs/SETUP.md`,
  `docs/CLI.md`, and `docs/SECURITY.md`.
- [x] Run and record the two-host Watch → detach → browser attach → input →
  graceful exit/kill → SQLite/log/audit acceptance on every available host —
  this file and the Milestone 1c report.
- [x] Run `just check`, Playwright, installer/launcher tests, update Roadmap
  1c, commit logical changes, and report exact installed/service paths.

## Open questions and resolved scope

- No new protocol type, dependency, Machine enrollment, aggregation service,
  or watcher process is required. The worker remains `sentinald`, `sentinal`,
  and the existing shell assets.
- The installer defaults to the current user's `~/.local/bin`, config, data,
  and native user-service directory. `--prefix` relocates program/service
  artifacts for offline tests but never implicitly relocates or deletes real
  user data.
- Service installation is the default on supported macOS/Linux hosts;
  `--no-service` installs only the worker files. Tests inject the platform and
  service commands and never invoke a real service manager.
- Uninstall retains config/data by default. Data removal requires a separate
  explicit flag containing the exact resolved data directory; a missing or
  mismatched value is refused.
- The real acceptance covers a macOS arm64 control Machine and an x86_64 Linux
  worker. A second macOS worker installation was exercised separately during
  private package bootstrap, but the complete checklist below used Linux.

## Acceptance record

- `just check` passed on macOS 27.0, including 365 fast Rust tests, 68 Vitest
  tests, TypeScript generation, and all four shell validation suites.
- `just test-e2e` passed: 3 Playwright scenarios use `VITE_DATA=api`, proxy to
  one fake worker, authenticate, attach over WebSocket, replay, and Continue.
- Real control Machine: macOS 27.0 arm64, Tailscale `100.93.101.56`.
- Real worker Machine: Pop!_OS 24.04, Linux
  `6.18.7-76061807-generic` x86_64, Tailscale `100.74.239.89`, verified worker
  release `0.1.5`. Tested repository commit:
  `7e3cc20831255d18ec74d1de58f8a8e4cfc41e4c`.
- The first run exposed `Linger=no`: closing the SSH login stopped the systemd
  user manager and correctly caused the restarted daemon to mark Session `1`
  `INTERRUPTED`. After `sudo loginctl enable-linger "$USER"`, daemon PID
  `58321` and Python PID `60021` survived CLI detach and SSH disconnect.
  `docs/SETUP.md` now documents this Linux prerequisite.
- `just control WORKER_IP=100.74.239.89` served the real API UI at
  `http://100.93.101.56:5173`. The owner completed bootstrap authentication,
  confirmed there was no `MOCK` badge, attached to Session `2`, observed live
  output, sent Ctrl-C, ran `echo SENTINAL_BROWSER_INPUT_OK`, and detached the
  pane.
- CLI reattach restored the screen including `SENTINAL_BROWSER_INPUT_OK`.
  `exit 0` made Session `2` `COMPLETED`. Its authenticated replay returned
  5,386 bytes and remained byte-identical in length after daemon restart.
- Session `3` was detached while its Python process ran, then
  `sentinal kill 3` made it `INTERRUPTED`. A repeated kill returned
  `Session #3 is in terminal state Interrupted and cannot kill a daemon-owned
  PTY`; no raw OS `EIO` appeared and no test process remained.
- SQLite stored Session `2` as `COMPLETED` with exit code 0, the browser marker
  Command with exit code 0, and the interrupted Python Command with exit code
  130. Session logs `2.log.zst` and `3.log.zst` were non-empty. Five audit
  records contained `byte_count`; neither the browser marker nor the long-job
  path appeared in `audit.jsonl`.
- Restarting `sentinald` changed PID `58321` to `62416`; health returned
  protocol version 1, Session states remained `INTERRUPTED`, `COMPLETED`, and
  `INTERRUPTED`, and finished output replay still contained the browser marker.

Commands used for the real acceptance (tokens were read locally and never
recorded here):

```console
worker$ sudo loginctl enable-linger "$USER"
worker$ loginctl show-user "$USER" -p Linger
worker$ sentinal update --check
worker$ sentinal update
control$ curl http://100.74.239.89:7420/api/v1/health
control$ just control WORKER_IP=100.74.239.89
worker$ sentinal watch -t "Handoff acceptance" -- python3 -u /tmp/sentinal-two-host-long-job.py
# Ctrl+J detached the first CLI.
worker$ sentinal ls
worker$ sentinal attach 2
worker$ sentinal watch -t "Kill acceptance" -- python3 -u /tmp/sentinal-two-host-long-job.py
# Ctrl+J detached the second CLI.
worker$ sentinal kill 3
worker$ sentinal kill 3
worker$ systemctl --user restart sentinald
worker$ sentinal ls
```
