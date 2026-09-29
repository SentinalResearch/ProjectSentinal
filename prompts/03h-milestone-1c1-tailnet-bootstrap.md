# MILESTONE 1c.1 — Tailnet bootstrap and private package delivery

Goal: install or upgrade a Sentinal worker on Linux or macOS from one trusted
Tailnet URL, start its user service, and leave it explicitly **UNPAIRED**.
Installation does **not** pair, enroll, name, or authorize the Machine.

Depends on completed Milestone 1c. Reuse `scripts/install-worker.sh`, the
existing `sentinal` + `sentinald` worker bundle, and the existing user-service
rendering. Do not create another agent daemon or implement `sentinal connect`
in this milestone.

Target experience:

```console
curl -fsSL https://sentinal-control.<tailnet>.ts.net/install.sh | sh

Sentinal worker installed and running
State: UNPAIRED
Next: sentinal connect https://sentinal-control.<tailnet>.ts.net
```

The documented reviewable alternative downloads the script first and then
runs it. The pipe form is convenience syntax, not a reason to weaken artifact
verification.

## Required design review — stop before tests or implementation

Present one concrete proposal covering:

1. Which existing `sentinald` instance is designated as the local control
   Machine and how package serving is enabled without creating a cloud
   service or a second daemon.
2. Tailnet-only HTTPS URL, Tailscale Serve/MagicDNS expectations, ACLs,
   validation of `100.64.0.0/10` addresses, and refusal of wildcard,
   ordinary-LAN, public, or plain-HTTP production binds.
3. Exact release bundle layout for Linux/macOS and supported architectures,
   including `sentinal`, `sentinald`, shell assets, service/config templates,
   version, and build metadata.
4. A bounded, versioned release manifest: artifact URL, length, SHA-256,
   target OS/architecture, minimum compatible protocol, and release channel.
5. Bootstrap-script trust and artifact verification. HTTPS authenticates the
   Tailnet control name; checksum verification detects corruption. If stronger
   signing needs a dependency outside the approved stack, stop for approval.
6. Publishing authority, atomic manifest/artifact replacement, rollback, and
   behavior when an upload or download is interrupted.
7. Installer behavior for fresh install, reinstall, downgrade refusal,
   explicit rollback, unsupported target, partial files, service-manager
   failure, and retained user data.
8. Why the install endpoint carries no Machine token, owner password, pairing
   code, or per-Machine credential in its URL, argv, output, manifest, or logs.
9. Rate/size bounds and audit metadata for package requests without recording
   client secrets or terminal data.
10. Compatibility with the existing offline/source-tree installer.

Show representative manifest JSON, exact URLs, directory layout, failure
messages, and the install/upgrade/rollback sequence. Then stop for explicit
human approval. This stage must not invent the enrollment wire contract.

## Implementation after approval

- Build reproducible worker release directories/archives from the workspace.
- Add a control-role package directory and bounded authenticated publishing
  path; downloads may be available to Tailnet ACL-authorized peers without a
  Sentinal Machine credential because they are not paired yet.
- Serve generated `install.sh`, the release manifest, checksums, and immutable
  versioned artifacts from the control Machine over the approved HTTPS path.
- Select only an exact supported OS/architecture. Never execute server-supplied
  shell fragments from the release manifest.
- Verify response status, declared length, and checksum before extracting or
  invoking the existing installer.
- Preserve config, Machine data, credentials, SQLite, logs, adapters, and
  recipes on every reinstall/upgrade.
- Start/enable the user service, but leave the worker unpaired. Print the
  separate `sentinal connect CONTROL_URL` next step.
- Add `sentinal update --check`, `sentinal update`, and explicit rollback only
  if they can reuse the same verified manifest path without enrollment state.
- Add `sentinal doctor` checks for binary/service/config/Tailscale health. It
  may report `UNPAIRED` but may not pair implicitly.

## Tests first

- Manifest parsing: every target, duplicate/missing fields, version/channel,
  incompatible protocol, excessive size, length mismatch, and bad checksum.
- Bootstrap shell tests with fake curl/service/install commands: Linux/macOS,
  supported architectures, spaces, interrupted download, HTTP error, corrupt
  archive, downgrade refusal, rollback, reinstall preservation, and cleanup.
- Package HTTP tests: exact immutable paths, traversal refusal, range/size
  bounds, missing release, atomic publish, and Tailnet-only production config.
- Security scans prove no token/password/pairing credential appears in URLs,
  argv, manifests, service files, logs, or audit records.
- Installer acceptance into temporary prefixes with no network or real service
  manager in fast tests.
- Ignored `tailnet_bootstrap_integration`: serve one release from a real
  control Machine, curl-install it on one Linux/macOS peer, verify service and
  `UNPAIRED`, then uninstall while retaining data.

## Done when

The displayed curl command installs a verified worker from the private
Tailnet package service, existing offline installation still works, the worker
is running but unpaired, `just check` passes, docs and Roadmap 1c.1 are
updated, the available-host integration is recorded, and the report is sent.
Commit and stop before Milestone 4.0 `sentinal connect` pairing.
