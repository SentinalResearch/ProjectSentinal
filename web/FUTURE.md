# Future — mock vs. real

Tracks what's backed by `src/data/mock` versus the real daemon API (`src/data/api`).
Update this whenever a feature moves from `mock` to `real`. See `docs/ROADMAP.md`
for when the underlying daemon features are expected to land.

Normal builds default to the real API. Fixture data is available only with
the explicit `VITE_DATA=mock` setting (normally `just demo`) and carries a
visible `MOCK` label. `just network` always forces API mode.

| Feature | Screen | Status | What's needed to make it real |
|---|---|---|---|
| Machines list | Overview, Settings | partial | `VITE_DATA=api` shows the connected daemon's real Machine; multi-machine aggregation waits for Roadmap M4 |
| Sessions list + live state | Overview, Session detail | real (M1) | `VITE_DATA=api` lists, creates, and polls real Sessions; daemon-pushed lifecycle events can replace polling later |
| Capture origin/badge/capabilities | Every Session surface | real (M1b) | Badges and explanations mirror daemon-enforced capabilities; reduced capture offers FULL Watch guidance |
| Watch / external shell / auto-wrap / adopted Sessions | Overview, Session detail | real (M1b/M3) | All four ingress modes are real; readable adopted logs become PROCESS_LOG and feed deterministic watchers |
| Terminal attach (input/output) | Session detail | real (M1) | `TerminalPane` mounts xterm.js and `useRealTerminalSession` attaches over the authenticated binary WebSocket; the Playwright smoke test covers the fake-daemon path |
| Health card (progress/activity/resources/stability/errors) | Session detail | real (M3) | Persisted deterministic Health with bounded evidence; richer adapter scalars can be added without AI |
| Event journal | Session detail, Project | real (M3) | Bounded persisted replay plus live Event broadcasts; project view combines Session timelines |
| Command history + search | History | real (M2) | API mode reads persisted Commands from SQLite and sends search terms through FTS5 |
| Resource metrics (CPU/RAM/GPU/VRAM) | Session detail | real (M2) | API mode streams per-Session process-tree totals; NVIDIA utilization/VRAM requires the daemon's `nvidia` feature, while watcher-derived loss remains M3 |
| Recipes (list + run) | Recipes | mock | Daemon: recipe files + launch API (M4) |
| Recipe editor / New recipe | Recipes | disabled | Not designed yet (brief: "currently disabled in the design") |
| Pairing flow (name → token → connect) | Settings | mock | Daemon: pairing/enrollment endpoint, `sentinal-agent pair` CLI |
| Rotate / revoke token | Settings | mock | Daemon: token management API |
| Auth (setup / login / lock / expire) | Auth | real (M1.1) | One Argon2id-backed owner account issues 24-hour browser sessions; setup, per-IP lockout, logout, expiry, and password rotation are daemon-enforced |
| Notification rules matrix | Settings | mock | Daemon: notification rule storage |
| Push notifications | Settings, phone | mock (placeholder UI, flag-gated) | Web Push subscription + daemon-side sender |
| "What happened while I was gone?" journal | — | mock (placeholder UI, flag-gated) | Depends on Event engine (M3) |
| Suggestion card (AI) | Session detail | mock (placeholder UI, flag-gated) | Intelligence layer (Roadmap V2), Permission Level 2 only |
| Ask box (NL → confirm action) | — | mock (placeholder UI, flag-gated) | Intelligence layer (Roadmap V2) |
| Phone screens (Round 2) | all | mock (placeholder UI, flag-gated) | Full design pass — only Foundations v2 §05 specimens exist today |
| Multi-browser session list | Settings → Account | mock (placeholder UI, flag-gated) | Daemon: session/device tracking |
| Ctrl-C interrupt (confirm → SIGINT) | Session detail | real (M1) | API mode sends byte `0x03`; hard kill sends the protocol `KILL` request, while mock mode only flips state |
| "Run again" (retry a failed command) | Session detail | not wired | No adapter method for it yet (see QUESTIONS.md #11) — needs a real retry/rerun endpoint before there's anything to call even in mock |

Legend: **mock** = fully working against `fixtures.json`, no real backend yet · **partial** = some of the underlying capability is real · **disabled** = intentionally not built (not designed) · **real** = wired to the daemon.
