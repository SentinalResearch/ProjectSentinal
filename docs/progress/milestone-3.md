# Milestone 3 progress — Event engine and watchers

Status: complete.

## Tasks and proof

- [x] Define the additive Health contract, delivery/persistence behavior, and
  bounded Event replay request — protocol unit tests, hand-written fixtures,
  every-kind coverage, and generated TypeScript checks.
- [x] Create the pure `crates/watcher` crate with bounded ANSI line framing,
  observations, adapter selection, rate limiting, and deterministic Health —
  watcher unit tests and snapshot tests.
- [x] Implement Generic, Training, pytest, vLLM/llama.cpp, cargo, and
  declarative TOML adapters — saved-log fixtures under
  `crates/watcher/tests/logs/` and reviewed `insta` snapshots.
- [x] Feed PTY output, Command lifecycle, process state, and Session Metrics
  into one per-Session event engine; persist and broadcast each Event and
  current Health — daemon FakePty/clock behavior tests.
- [x] Tail adopted logs read-only with append/truncate/rotation/partial-line
  handling and bounded buffers; upgrade active capture to `PROCESS_LOG` —
  daemon temp-file tests plus ignored real-log integration.
- [x] Replace real-mode UI placeholders with structured Event detail,
  persisted timelines, deterministic Health cards, progress, and capture
  guidance — adapter Vitest, component tests, and Playwright training replay.
- [x] Update architecture, security, dependency status, Roadmap 3/1b.D, run
  `just check` and Playwright, commit logical changes, and send the milestone
  report.

## Proposed additive wire decision

Keep `PROTOCOL_VERSION = 1` and preserve all existing fixtures with defaults.

```rust
pub enum HealthLevel { Unknown, Good, Warning, Critical }

pub enum HealthOverall { Healthy, Warning, PossiblyStalled, Failed }

pub struct Health {
    pub progress: Option<f32>, // percent, clamped to 0..=100
    pub activity: HealthLevel,
    pub resources: HealthLevel,
    pub stability: HealthLevel,
    pub errors: u32,
    pub overall: HealthOverall,
    pub evidence: Vec<String>,
}
```

Add `health: Option<Health>` to `Session` with a Serde default and omission
when absent. This makes current Health available in `LIST_SESSIONS`, after a
browser reload, and after daemon restart without another request. Health is a
derived cache: the watcher is authoritative, while each update is persisted
with the Session and broadcast through the existing `SESSION_UPDATED` message.
Evidence is bounded to 16 entries of 512 UTF-8 bytes each before persistence.

Add this request for bounded replay of persisted Events:

```rust
ClientRequest::GetEvents {
    session_id: Option<SessionId>,
    after: Option<EventId>,
    limit: u32,
}
```

Reuse the existing `DaemonMessage::Event { event }` for live Events and add:

```rust
DaemonMessage::Events { events: Vec<Event> }
```

`limit` is `1..=500`; results are oldest first and filtered by Session when
provided. The web loads a Session's bounded persisted timeline, then appends
live `EVENT` messages by Event ID without parsing terminal output. No terminal
text is added to Session or request payloads.

## Approved decisions

1. The common `UNKNOWN | GOOD | WARNING | CRITICAL` Health level
   scale, rather than context-specific strings such as `LOW`, `IDLE`, or
   `PAUSED`.
2. Current Health is an optional persisted Session field and reuses
   `SESSION_UPDATED`, rather than a transient `HEALTH_UPDATED` message that
   would disappear on reload/restart.
3. Bounded `GET_EVENTS` / `EVENTS` additions let the real UI rebuild
   an Event timeline after reload, rather than displaying only Events observed
   while the WebSocket is open.

No dependency outside the approved stack is proposed. `regex` and `insta`
move from the Milestone 3 approved list into use; `toml` is already in use.

## Completed outcome

- PTY output, shell Command transitions, process samples, and Session Metrics
  now feed a pure per-Session deterministic Event engine. Events are persisted
  in SQLite and streamed to clients; current Health is persisted on Session.
- ANSI-free framing is streaming and bounded to 8 KiB, including final output
  without a newline. Progress is limited to one Event per five seconds unless
  it moves by at least one percentage point.
- Built-in adapters cover generic prompts/errors/stalls, Python training,
  pytest, vLLM/llama.cpp servers, and Cargo. Declarative TOML adapters load in
  deterministic filename order with file/rule-specific validation errors.
- Adopted logs are tailed read-only through the same watcher pipeline, with
  bounded reads and append, truncation, and rotation handling.
- The real-data web adapter reloads bounded persisted Events and renders live
  timeline detail, progress, deterministic Health, and capture guidance.
- `just check` passes. All three Playwright smoke tests pass. The ignored
  `watch_integration` was run manually on macOS with a real `sentinald`, PTY,
  shell, and Python process; it proved progress/checkpoint Events and Health,
  client-close survival, same-PID reattach, and explicit cleanup.
