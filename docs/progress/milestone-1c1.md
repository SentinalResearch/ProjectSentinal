# Milestone 1c.1 progress — Tailnet bootstrap and private package delivery

Status: complete 2026-09-30.

## Approved design

- The owner enables a package role on the existing control `sentinald`. It
  uses a second loopback-only listener and is exposed to selected Tailnet
  peers by Tailscale Serve over HTTPS. It is not a second daemon and never
  uses Funnel.
- The package API is additive and independent of protocol version 1:
  `GET /install.sh`, `GET /packages/v1/channels/stable.json`, immutable
  `GET /packages/v1/releases/<version>.json`, and immutable
  `GET /packages/v1/artifacts/<file>` resources.
- Publishing is local and authorized by OS ownership of the private package
  directory. There is no remote upload endpoint.
- Manifests are bounded to 64 KiB and 16 targets. Artifacts are bounded to
  256 MiB each and contain an allowlisted worker layout. The bootstrap checks
  target, protocol compatibility, declared length, archive paths/types, and
  SHA-256 before invoking the existing offline installer.
- Package downloads have no Sentinal bearer or pairing credential. Their
  authorization boundary is the loopback-only listener plus Tailscale Serve
  and an owner-configured Tailnet Grant. Package URLs, files, logs, and
  service definitions contain no token, password, pairing code, or Machine
  credential.
- Installation and update preserve configuration, credentials, SQLite,
  Session logs, audits, adapters, and recipes. The worker ends explicitly
  `UNPAIRED`; stable Machine identity and `sentinal connect` remain Milestone
  4.0 work.
- No protocol fixture, protocol version, or dependency changes are approved.

## Tasks and proof

- [x] Add release-manifest parsing and validation, target selection, version
  comparison, length and SHA-256 verification — Rust/unit and shell tests.
- [x] Build reproducible worker release directories and archives for exact
  supported targets without fabricating unavailable cross-target builds —
  `scripts/tests/package-release-test.sh`.
- [x] Publish immutable artifacts and manifests atomically, with an atomic
  stable pointer and explicit rollback — `scripts/tests/package-release-test.sh`.
- [x] Add the loopback-only package listener, bounded static routing,
  traversal/range refusal, download concurrency bounds, and metadata-only
  audit records — `crates/daemon/tests/package_service.rs`.
- [x] Generate and serve a bootstrap script that selects an exact target,
  downloads without redirects, verifies length and SHA-256, validates archive
  entries, and invokes the existing installer —
  `scripts/tests/package-release-test.sh`.
- [x] Preserve the checked-out source/prebuilt offline installer and add
  transactional service failure handling plus release metadata — existing
  and expanded installer shell tests.
- [x] Add `sentinal doctor` without enrollment side effects — CLI unit and
  behavior tests.
- [x] Document Tailscale Serve/Grant setup, curl and reviewable installation,
  update/rollback, failures, and the explicit UNPAIRED boundary — setup,
  security, dependency, and CLI documentation tests.
- [x] Run `just check`, update this checklist and Roadmap, and prepare the
  logical milestone commit and report.
- [x] Complete the manual real-host equivalent of
  `tailnet_bootstrap_integration` from a real control Machine onto an
  authorized Linux peer and record service/UNPAIRED/uninstall evidence.

## Open questions and fixed scope

- Independent artifact signing is outside this approved design. HTTPS
  authenticates the Tailnet name and SHA-256 detects corruption, but neither
  protects against a compromised control Machine or publisher.
- Fast tests use fake curl, archive, service, and filesystem inputs. They do
  not need network access or a real service manager.
- `sentinal connect`, Machine naming, enrollment credentials, aggregation,
  and a permanent Machine identity are explicitly excluded.
- Milestone 1c's separate real two-host Watch handoff remains unrecorded and
  is not satisfied by package-service tests.

## Acceptance record

- `just check` passed on macOS on 2026-09-30: Rust fmt/clippy/tests, 68
  Vitest tests, and all six launcher/installer/package shell suites passed.
- Fast package-service tests cover all four supported OS/architecture targets,
  manifest failures, bounded routes, traversal and symlink rejection,
  metadata-only audit, atomic publication, update, rollback, and preservation.
- Real control: arm64 macOS 27.0 at `100.93.101.56`, Tailnet HTTPS origin
  `https://advanceds-mac-mini-1.tail17dafb.ts.net`, with Funnel disabled and
  package HTTP bound only to `127.0.0.1:7422`.
- Real peer: x86_64 Pop!_OS 24.04 (kernel 6.18.7) at `100.74.239.89`.
  Release `0.1.1` downloaded over Tailnet HTTPS, matched manifest length and
  SHA-256, installed to `~/.local`, enabled `sentinald.service`, and reported
  binaries/config/Tailscale/service `OK` plus `Pairing: UNPAIRED`.
- The control Machine reached `http://100.74.239.89:7420/api/v1/health`; the
  worker reported `Sentinal 0.1.1 is current` from `sentinal update --check`.
- Uninstall removed programs/service while retaining the exact config and data
  directories and their inodes. Reinstall from the same HTTPS `install.sh`
  restored an enabled, active service and remained `UNPAIRED`.
- Real-host acceptance found and fixed four portability/reliability defects:
  GNU/BSD `stat` probing, recursive tar-list duplicate hardlinks and unsafe
  modes, Bash installer invocation/default-prefix systemd placement, and a
  launchd bootout/bootstrap race. Regression tests cover each path.
