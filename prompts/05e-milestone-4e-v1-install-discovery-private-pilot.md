# MILESTONE 4.E — V1 installation, discovery, and private pilot

Goal: a person who understands neither Linux services nor Sentinal internals
can install one control and multiple workers, discover the control over their
Tailnet, approve each Machine with one click, receive safe updates, and use a
single-tenant production pilot hosted on a small cloud VM.

This milestone completes V1 distribution and onboarding before any native
mobile application work. It does not create a public terminal service,
multi-tenant SaaS, organization accounts, billing, a vendor relay, silent
pairing, or a second agent daemon. The worker remains `sentinald` plus the
`sentinal` CLI; the deterministic watcher remains linked into `sentinald`.

Depends on completed Milestones 1c, 1c.1, 4.0, 4.A, 4.B, and 4.D. Reuse the
existing verified manifest, atomic installer, launchd/systemd user services,
Machine identity, directional link credentials, registry, revocation,
production PWA, and private Tailnet HTTPS origin. Preserve the rule that
installation and authorization are separate ceremonies.

Before changing code, read `AGENTS.md`, `docs/ARCHITECTURE.md`,
`docs/DEPENDENCIES.md`, `docs/SECURITY.md`, `docs/SETUP.md`, `docs/CLI.md`,
`docs/AUDIT_TRACKER.md`, `docs/progress/milestone-1c.md`,
`docs/progress/milestone-1c1.md`, `docs/progress/milestone-4.0.md`, the current
installer/package scripts, Machine-link routes, and Tailscale launch helpers.
Add Milestone 4.E to `docs/ROADMAP.md` and create
`docs/progress/milestone-4e.md` with tasks, proving tests, open questions,
release/signing evidence, and the exact real-Machine pilot matrix.

## Product experience

The normal path must look like this:

```text
Install Sentinal
      ↓
Choose this Machine's role
  • Create my Sentinal control
  • Connect to my existing Sentinal
      ↓
Worker discovers verified controls on the Tailnet
      ↓
Choose “Advanced’s Sentinal”
      ↓
Control shows “Sohith’s MacBook wants to connect”
      ↓
Compare a short verification phrase and press Approve
      ↓
Connected · service running · updates configured
```

The fallback path accepts an explicitly entered verified control URL and the
existing hidden one-time code flow. No normal path requires copying a token,
editing TOML, learning a port, enabling a service manually, or placing a
pairing secret in argv, a URL, environment, browser storage, logs, or shell
history.

## Required design review — stop before tests or implementation

Present one concrete design covering all of these points:

1. **Release trust and initial bootstrap.** Define how the first control is
   obtained before a private control package service exists. Choose one
   reviewable V1 trust root—signed static HTTPS/GitHub-style releases or a
   locally transferred verified release—and distinguish it from the Tailnet
   package service used after bootstrap. Specify manifest signing, checksum,
   key storage, rotation, compromise recovery, artifact provenance, and
   rollback. HTTPS plus a checksum served by the same origin is not an
   independent signature. Stop if signing/notarization requires an unapproved
   dependency, external account, paid credential, or CI authority.
2. **Supported installers.** Propose exact deliverables for macOS and Linux:
   signed/notarized `.pkg` when credentials are available, `.deb` for
   Debian/Ubuntu/Pop!_OS, verified release archive, and `curl | sh` fallback.
   Explain whether Homebrew and an apt repository are implemented now or
   deferred. Do not use `pip` for a Rust daemon, native binaries, shell assets,
   and system services. Show supported OS/architecture/version combinations
   and honest failure messages for every unsupported target.
3. **Interactive setup.** Define `sentinal setup` as an idempotent guided flow
   for choosing CONTROL or WORKER, detecting Tailscale, installing/enabling the
   user service, creating only missing configuration, checking health, and
   explaining the next action. Specify protected password input, cancellation,
   rerun/recovery, noninteractive automation flags, exit codes, accessibility,
   and what requires administrator privileges. Existing config/data must never
   be silently replaced.
4. **Tailscale discovery.** Discover only peers returned by the local
   Tailscale client/status API; never scan `100.64.0.0/10`. Prefer verified
   MagicDNS HTTPS names and bounded concurrent probes to a minimal discovery
   resource. Define timeouts, redirects, DNS/IP validation, duplicate names,
   several controls, offline peers, older versions, ACL refusal, missing
   Tailscale, macOS app-bundle CLI discovery, and manual fallback. mDNS must not
   be presented as reliable across a Tailnet.
5. **Discovery resource.** Show the exact unauthenticated Tailnet-only response
   containing no token, password state, Machine credential, Session data,
   subscription, terminal metadata, filesystem path, or user list. Define its
   bounds, rate limits, cache policy, version compatibility, Tailscale Serve
   requirement, and why it cannot be used to reach a public/wildcard daemon.
6. **Approval pairing contract.** Define request, status polling, approval,
   rejection, expiry, cancellation, replay protection, and credential exchange
   for click-to-approve pairing. A pending request must be bound to the
   requesting Machine identity and a worker-held secret or ephemeral key so a
   second peer cannot claim the approval. The control derives Machine identity
   from authenticated proof rather than trusting a claimed ID. Show all JSON,
   storage, generated TypeScript impact, and audit metadata.
7. **Human verification and authority.** Display the same short verification
   phrase on worker and control before approval. Define entropy/derivation,
   collision scope, expiry, retry limits, concurrent requests, wrong-control
   protection, and behavior when no display is available. Pairing is never
   silent or implied by installation, Tailnet membership, hostname, or package
   download. Preserve the existing hidden one-time-code flow as fallback.
8. **Control role and URLs.** Define one stable control identity and canonical
   Tailnet HTTPS origin. `sentinal setup` may configure Tailscale Serve only
   after an explicit preview/confirmation and must never enable Funnel, bind a
   wildcard/public/LAN address, or overwrite unrelated Serve routes. Define
   origin changes, hostname changes, certificates, restore, and how workers
   verify they are reconnecting to the same control.
9. **Private cloud pilot.** Specify one single-owner control deployed on a
   small Linux cloud VM joined to a dedicated/personal Tailnet. `sentinald`
   binds loopback behind Tailscale Serve; provider firewall rules expose no
   Sentinal API/package/terminal port publicly. Define minimum resources,
   service persistence/linger, backup/restore, monitoring, log rotation,
   outbound Internet needs for Web Push and updates, VM loss/rebuild, and
   Tailnet ACL examples. A public landing/download site, if desired, is a
   separate static origin with no API, credentials, terminal data, or control
   proxy and requires separate approval.
10. **Private package manager.** Define how the control publishes immutable
    stable/beta releases, how workers learn update availability, exact
    OS/architecture selection, signature/checksum/length verification,
    bandwidth and retention bounds, atomic channel changes, rollback, partial
    download recovery, and offline behavior. Package download grants no pairing
    authority.
11. **Update coordination.** The control UI may offer Update only with explicit
    owner confirmation, correct Machine scoping, version/status display, and a
    warning that daemon restart interrupts live FULL Sessions until Reliability
    R1. Define worker acknowledgement, timeout, refusal with live Sessions,
    staged update, health check, automatic rollback, mixed-version behavior,
    and one failed Machine not affecting another.
12. **Install/uninstall/data lifecycle.** Preserve Machine identity,
    credentials, password hash, VAPID key, browser subscriptions, SQLite,
    Session logs, recipes, adapters, and configuration across reinstall and
    upgrade. Uninstall removes programs/services but retains data by default.
    Destructive removal requires exact-path display and explicit confirmation.
    Define backup and restore boundaries for control and workers.
13. **Enterprise-pilot truth.** This is a private single-tenant pilot, not an
    enterprise identity platform. Document missing SSO, RBAC, organizations,
    tenant isolation, compliance certification, HA, billing, public ingress,
    and support SLAs. Do not label the pilot production enterprise software.
14. **Compatibility and dependencies.** Prefer additive protocol/HTTP changes
    and keep `PROTOCOL_VERSION = 1` where possible. Show legacy worker behavior,
    downgrade behavior, fixture changes, database migrations, generated TS,
    new dependencies/tools/licenses, and which signing steps cannot run in CI.

Show representative discovery, pending-pairing, approval, rejection, expiry,
and update JSON; CLI screens for control setup and worker setup; package names
and layouts; the complete trust/data flow; VM topology; important failure
messages; and the rollback/recovery sequence. Then stop for explicit human
approval. Do not change protocol fixtures, install signing keys, provision a
real VM, modify Tailscale Serve/ACLs, publish public artifacts, or implement
production code before approval.

If the design cannot remain additive, report the compatibility break and stop.

## 1. Reproducible installable releases

After approval:

- Produce reproducible release archives for every approved target and package
  them into the approved macOS/Linux installer formats without embedding
  credentials, hostnames, Tailnet details, or user paths.
- Reuse `scripts/install-worker.sh` as the installation core so archive,
  `.pkg`, `.deb`, and private-update paths preserve identical safety behavior.
- Verify independent signatures, manifest length/checksum, target, version,
  minimum protocol, and expected file ownership before installation.
- Install `sentinal`, `sentinald`, shell assets, production PWA assets, service
  templates, and release metadata. Do not install a second watcher process.
- Preserve the current offline/source-tree installer for development and
  recovery.

## 2. Guided role setup

- Add an idempotent `sentinal setup` that clearly offers CONTROL, WORKER, and
  advanced/manual paths.
- CONTROL setup previews exact bind, Tailnet hostname, Serve routes, data
  paths, service file, package directory, PWA URL, backup boundary, and restart
  effect before applying anything.
- WORKER setup discovers controls, allows one explicit selection, creates a
  pending approval request, displays verification words, waits with a bounded
  deadline, and finishes only after credentials are durably stored and the
  authenticated heartbeat succeeds.
- A failed/cancelled setup leaves no half-link, leaked credential, broken
  service, or replaced configuration. It prints one actionable recovery step.
- `sentinal doctor` reports installation, role, Tailscale, service, control
  reachability, pairing, package channel, version, and update state without
  printing secrets or silently changing the Machine.

## 3. Discovery and approval pairing

- Add the approved bounded control discovery resource and Tailnet peer
  discovery client. Every network operation has injected resolution/transport
  seams for offline tests; do not add a trait without two implementations.
- Persist pending requests with expiry and one-success semantics. Approve and
  reject from the authenticated owner PWA; show safe Machine metadata and the
  verification phrase.
- Issue existing directional credentials only after approval. Keep all
  Machine-scoped routing, heartbeat, refresh, disconnect, and revocation
  behavior unchanged after pairing.
- Audit request, approve, reject, cancel, expire, collision/replay refusal, and
  failed claims with IDs/reasons but no secrets, full request bodies, Tailnet
  auth headers, or terminal data.
- Show pending/connected/revoked/offline/update states accessibly at 390×844
  and 320×568.

## 4. Private package and update UX

- Add one bounded same-user publishing command that builds/verifies all
  supplied target artifacts before atomically changing a channel pointer.
- Show current and available release per Machine without sending a worker
  credential to the browser.
- Update one selected Machine through the existing authenticated link only
  after confirmation. The worker independently verifies the approved release;
  the control cannot ask it to execute arbitrary text or a caller path.
- Keep old releases according to the approved retention bound and make rollback
  explicit, signed, auditable, and Machine-scoped.
- Failed install/update/service health checks restore the previous program
  bundle and leave user data untouched.

## 5. Cloud pilot deployment

- Add an offline-reviewable deployment/runbook for one supported Linux VM. Use
  declarative config/templates already in the repository where possible; do
  not add Terraform, Docker, Kubernetes, or a cloud SDK without separate
  approval and dependency documentation.
- Require the operator to join the VM to Tailscale explicitly. Validate the
  resulting identity/address, then configure only loopback listeners and the
  reviewed Tailscale Serve routes.
- Add safe preflight and inspection commands for firewall exposure, Serve
  configuration, service status, HTTPS health, PWA, package service, Web Push,
  backup freshness, disk use, and update channel.
- Document a pilot reset/rebuild that restores stable control identity and
  credentials from an owner-controlled backup or intentionally creates a new
  control and re-pairs workers.
- Never commit cloud credentials, Tailnet auth keys, DNS names, public IPs,
  signing keys, owner passwords, pairing secrets, or real Machine credentials.

## Tests first

- **Installer/package:** every supported target; fresh install, reinstall,
  upgrade, explicit downgrade/rollback, interrupted/corrupt download, wrong
  signature/checksum/length/target/protocol, unsafe source permissions, paths
  with spaces, service failure rollback, retained data, and exact-confirmation
  uninstall. Fast tests use temporary prefixes and fake signing/service tools.
- **Setup:** CONTROL/WORKER/manual choices, cancellation at every step,
  idempotent rerun, missing Tailscale, macOS app-bundle CLI, unsupported OS,
  noninteractive flags, inaccessible config, existing role conflict, and
  actionable exit codes. No real service manager in fast tests.
- **Discovery:** zero/one/many controls, duplicate names, offline peer, ACL
  refusal, timeout, redirect, invalid certificate/origin, public/private/LAN
  endpoint refusal, incompatible version, malformed/oversized response, bounded
  concurrency, and no whole-CIDR scan.
- **Pairing contract:** request/approve/reject/cancel/expire, one-success race,
  request-secret or ephemeral-key proof, verification phrase agreement,
  replay/wrong claimant/wrong control, owner authentication, rate limits,
  credential redaction, audit privacy, fallback code compatibility, fixtures,
  migration, and generated TypeScript.
- **Update:** availability, mixed worker versions, live-Session warning/refusal,
  confirmation, authenticated Machine scope, independent worker verification,
  timeout, worker offline, health failure rollback, control restart, and one
  failed worker isolated from another.
- **Web:** first-run/pending approval/update UX, keyboard and screen-reader
  labels, honest progress/errors, 390×844 and 320×568, no credential in browser
  storage, and no mock data in production.
- **Playwright:** one production control plus two fake clean workers: discover,
  request, compare phrase, approve, heartbeat, rename, update one, reject one,
  revoke/re-pair, and preserve exact Machine routing.
- **Security:** sweep artifacts, manifests, installer output, argv, environment,
  service files, browser state, logs, audits, URLs, and release metadata for
  planted passwords, link credentials, tokens, pairing/request secrets,
  terminal output, Commands, and private paths.
- Fast tests use no public network, cloud account, real signing credential,
  real service manager, or sleeps over 100 ms. Real packaging/signing,
  Tailscale, and VM behavior belong to explicit ignored/manual acceptance.

## Real-Machine and private-pilot acceptance

Run against one exact candidate commit and record sanitized literal evidence in
`docs/progress/milestone-4e.md`:

1. Build and independently verify the candidate artifacts for the available
   macOS and Linux architectures. Record unsigned/unnotarized coverage honestly
   when production credentials are unavailable.
2. Provision one small Linux VM, join it to the pilot Tailnet, prove no
   Sentinal port is publicly reachable, and initialize it as the single control
   behind Tailscale Serve.
3. From clean-state macOS and Pop!_OS/Ubuntu workers, use the normal installer
   and `sentinal setup` without editing TOML or invoking launchctl/systemctl.
4. Discover the VM control, request connection, compare verification words,
   approve in the production PWA, and observe authenticated heartbeats.
5. Run Watch on each worker, close the first client, attach from the control,
   produce a structured Event, receive Web Push, and open the exact namespaced
   Session.
6. Publish one newer candidate, update one worker, verify data preservation and
   mixed-version operation, then exercise explicit rollback. Record the honest
   FULL-Session interruption until Reliability R1.
7. Reinstall and uninstall one worker, proving program removal and default data
   retention. Reconnect/recover without duplicating Machine identity.
8. Restart and back up the VM control, simulate worker/control offline states,
   and validate recovery. Test restore only with sanitized disposable pilot
   data.
9. Inspect firewall, Tailscale Serve, files, permissions, SQLite, logs, audits,
   browser storage, and artifacts for forbidden public exposure or secrets.

## Done when

A non-expert can install a private V1 control and workers using the approved
normal installer, run `sentinal setup`, discover the correct Tailnet control,
approve Machines with matching verification words, update/rollback safely, and
use the production pilot without editing configuration or handling a Machine
credential. The cloud VM exposes no Sentinal port publicly; pairing remains
explicit; existing code pairing and offline installation remain available;
`just acceptance` passes; the available real-Machine/signing/VM matrix is
recorded; and the required milestone report is sent. Stop before a native
mobile app, public SaaS, multi-tenant enterprise work, or Reliability R1.
