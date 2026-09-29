# MILESTONE 1c — Deployable worker and two-host acceptance

Goal: install one Sentinal worker Machine, run the real control UI from a
second Machine, and prove Watch handoff over Tailscale without pretending that
multi-Machine aggregation or pairing already exists.

Depends on completed Milestone 1b.E. This milestone is deployment and
acceptance for exactly one selected worker daemon. It does not create a
central server, Machine enrollment protocol, or `crates/watcher`.

## 1. Development launch roles

Add and test these commands while preserving `just network` for a combined
same-Machine setup:

```console
just agent
just control WORKER_IP=100.x.y.z
```

- `just agent` starts only `sentinald`, bound to this Machine's exact
  Tailscale IPv4 address. It uses persistent config/data/token/auth paths and
  prints the health URL, socket path, token bootstrap location, and stop
  behavior.
- `just control WORKER_IP=...` starts only the Vite UI on the control
  Machine's exact Tailscale IPv4 and proxies HTTP/WebSocket API traffic to the
  selected worker. It always forces `VITE_DATA=api`, even if the caller's
  environment says `mock`.
- Both roles reject `0.0.0.0`, `::`, ordinary LAN addresses, malformed ports,
  and a worker address outside `100.64.0.0/10`.
- Neither role silently falls back to fixtures or localhost.
- Reuse shared shell functions rather than copying the current network
  launcher's validation and quoting logic into three scripts.

## 2. Worker package and local release installer

- The worker package is `sentinald` + `sentinal` + approved shell-hook assets
  + service/config templates. Do not invent a second “watcher agent” daemon.
  Milestone 3's deterministic watcher is a library linked into `sentinald`.
- Provide an offline-testable release installer from a checked-out source tree
  or prebuilt release directory. Network download/signing automation is not
  implied by this milestone.
- Default to a per-user install (`~/.local/bin` or the platform's documented
  equivalent), and refuse an unsafe/writable-by-others source binary.
- Generate a config bound only to an explicitly selected or validated
  Tailscale address. Preserve existing token, password hash, SQLite, logs,
  adapters, and recipes on reinstall.
- Install and enable the existing systemd user unit on Linux or launchd user
  agent on macOS. Templates must use the actual installed paths and must not
  embed the Machine token.
- Upgrade uses an atomic binary replacement where the platform permits.
  Document that FULL Sessions are interrupted by daemon restart until
  Reliability R1 lands.
- Uninstall stops/disables the service and removes installed program files.
  User data/config remain by default; data deletion requires a separate,
  explicit confirmation and exact path display.
- `--dry-run`, `--prefix`, and non-service installation are supported so tests
  never write to real user service directories.

Stop if packaging safely requires a dependency outside the approved stack.

## 3. Setup and operations documentation

Create `docs/SETUP.md` covering:

- prerequisites and Tailscale ACL/firewall expectations;
- worker install, first start, password bootstrap, service status, logs,
  restart, upgrade, uninstall, and backup;
- same-Machine `just network`, worker `just agent`, and remote UI
  `just control WORKER_IP=...` with exact commands;
- why the daemon never binds a wildcard or ordinary LAN address;
- how to inspect health, SQLite, compressed Session logs, and metadata-only
  audit logs without printing credentials;
- current boundaries: one selected worker, no enrollment/aggregation, no
  attachable PTY survival across daemon restart.

Update `docs/CLI.md`, `docs/SECURITY.md`, packaging comments, and service
inspection commands (`sentinald`, not only the compatibility alias).

## Tests first

- Shell tests for `agent`, `control`, and combined launch dry-runs, including
  forced `VITE_DATA=api`, spaces in paths, all rejected bind/worker addresses,
  occupied ports, missing Tailscale, and child-process cleanup.
- Installer tests into a temporary prefix: fresh install, reinstall, upgrade,
  dry-run, unsupported OS, missing asset, wrong permissions, uninstall with
  retained data, and explicit data-removal confirmation refusal.
- Service rendering snapshots for systemd and launchd with paths containing
  spaces; no token/password appears in rendered files.
- Web smoke test through a control proxy against one fake worker, including
  WebSocket attach and browser authentication.
- Integration `two_host_acceptance` remains documented/manual: on two real
  Tailnet Machines run Watch → detach/close → browser attach → input → graceful
  exit and forced kill → inspect SQLite/log/audit. Record OS versions and exact
  commands in the milestone report.

Fast tests use fake commands/processes and no network access or real service
manager.

## Done when

One worker can be installed and controlled from a second Machine, every normal
launch path uses real API data, the two-host checklist is completed on the
available hosts (with unavailable platform coverage named), `just check` and
Playwright pass, Roadmap 1c is checked, and the report lists installed paths,
service names, preservation behavior, and known restart limitation. Commit and
stop before Milestone 3.
