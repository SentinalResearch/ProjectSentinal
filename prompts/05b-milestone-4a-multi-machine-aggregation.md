# MILESTONE 4.A — Multi-Machine aggregation and routing

Goal: one PWA shows every enrolled Machine and routes each read/control action
only to the daemon that owns the selected Session.

Depends on completed Milestone 4.0 enrollment. Keep the direct local-first
architecture: the browser connects independently to per-Machine APIs; no cloud
service is required.

## 1. Multi-Machine data model

- Replace the singleton daemon assumption with a `MachineConnection` per
  enrolled Machine: stable ID, display name, base HTTP/WS URLs, credential
  reference, connection state, last successful contact, capabilities, and
  bounded retry state.
- Namespace every client-side Session, Command, Event, Metric, and pending
  request by Machine ID. Numeric Session ID `1` on two Machines must never
  collide.
- Keep protocol IDs unchanged on each daemon. Cross-Machine composite IDs are
  a web/control-plane concern unless the approved design review proves a wire
  change necessary.
- Query Machines concurrently with a configured limit and isolate failures.
  One slow/offline daemon must not block or clear healthy Machine data.
- Use deterministic retry/backoff with injected clocks and no retry storm when
  the browser resumes from sleep.

## 2. Routing and lifecycle

- Route list/history/search/Metrics/Event subscriptions, terminal attach,
  binary input, resize, Ctrl-C, kill, and later Recipe execution through the
  owning `MachineConnection` only.
- A terminal WebSocket uses that Machine's URL and credential. Never send a
  credential for Machine A to Machine B or to the control UI origin unless it
  is Machine A's approved origin.
- Offline Machines retain their last known structured snapshot, clearly
  labelled `OFFLINE` with last-seen time. Do not relabel persisted terminal
  Session state as `DISCONNECTED` on disk merely because this browser is
  offline.
- On reconnect, replace stale snapshots atomically and resume subscriptions
  without duplicating Events.
- Logout/remove/revoke affects one Machine unless the user explicitly chooses
  all Machines.

## 3. UI

- Overview groups Sessions by Machine and shows online/offline/auth-expired
  states separately.
- Every Session surface and destructive confirmation names its Machine.
- Search can select one Machine or all online Machines and reports partial
  results when some are unavailable.
- Split view may contain Sessions from different Machines; each pane exposes
  its connection state independently.
- Settings lists enrolled Machines, URL, last seen, capabilities, credential
  status, rename, reconnect, and revoke/remove actions.
- Preserve phone layout and accessible keyboard behavior.

## Tests first

- Unit: composite identity collisions, routing table, partial failures,
  concurrent query limit, deterministic backoff, wake/resume, credential
  isolation, Event deduplication, and one-Machine logout.
- Adapter: two fake WebSocket/HTTP daemons both using Session ID `1`; assert
  attach/input/kill/history/search reach only the selected owner.
- UI: grouped online/offline/auth-expired states, last seen, partial search,
  cross-Machine split view, and Machine-named confirmations.
- Playwright: two fake daemons; create/attach on both, take one offline, keep
  controlling the other, restore it, and verify no duplicate Events.
- Security: capture all fake requests and prove no cross-Machine credential or
  terminal frame leakage.
- Manual real acceptance on two Tailnet Machines with exact commands and OS
  versions recorded in the report.

No real network or sleeps over 100 ms in fast tests.

## Done when

The PWA safely shows and controls two real enrolled Machines, an offline or
expired Machine cannot break another, `just check` and Playwright pass,
Roadmap 4.A is checked, and the report contains the two-Machine routing and
credential-isolation evidence. Commit and stop before Milestone 4.B.
