# Milestone 1c progress — Deployable worker and two-host acceptance

Status: in progress 2026-09-29.

## Tasks and proof

- [x] Extract shared Tailscale/address/port/config helpers and add distinct
  combined, agent-only, and control-only launch roles —
  `scripts/tests/run-roles-test.sh`.
- [x] Add `just agent` and `just control WORKER_IP=...`, force real API mode,
  reject unsafe addresses and occupied ports, and clean up child processes —
  `scripts/tests/run-roles-test.sh`.
- [ ] Add an offline per-user worker installer for source trees and prebuilt
  release directories, including dry-run, reinstall/upgrade, non-service
  mode, safe source validation, and data-preserving uninstall —
  `scripts/tests/install-worker-test.sh`.
- [ ] Render systemd and launchd user services with exact installed paths and
  no credentials, including paths containing spaces —
  `scripts/tests/install-worker-test.sh` snapshots/assertions.
- [ ] Prove browser authentication and WebSocket attach through the control
  proxy against the fake daemon — `web/e2e/session-smoke.spec.ts` through the
  control-role Playwright server configuration.
- [ ] Document setup, service operations, backup/recovery, inspection, current
  boundaries, and the manual two-host checklist — `docs/SETUP.md`,
  `docs/CLI.md`, and `docs/SECURITY.md`.
- [ ] Run and record the two-host Watch → detach → browser attach → input →
  graceful exit/kill → SQLite/log/audit acceptance on every available host —
  this file and the Milestone 1c report.
- [ ] Run `just check`, Playwright, installer/launcher tests, update Roadmap
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
- This development host is macOS. A second Tailnet Machine and Linux host are
  not assumed available; missing manual coverage will be named rather than
  simulated or claimed.
