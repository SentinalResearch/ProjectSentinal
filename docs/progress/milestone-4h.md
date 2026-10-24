# Milestone 4.H — measurements from the owner's Tailnet

## Baseline (first report, 2026-10-24)

From the owner's first downloaded report (`range=24h`, `names=anonymous`),
taken a few hours after the control was updated to 0.2.6. The workers still
ran 0.2.3–0.2.5, so they do not report their own memory/CPU yet; their rows
come from the control's measurements and the browser.

| Machine | Role / OS | Link round trip p50 / p95 | Terminal setup (control) p50 | Terminal open (browser) p50 | Typing echo p50 / p95 |
|---|---|---|---|---|---|
| Machine 1 | control, Linux | — | — | — | — |
| Machine 2 | worker, Linux (US) | 117 / 117 ms | 469 ms | 802 ms | 116 / 234 ms |
| Machine 3 | worker, macOS | 6 / 69 ms | 24 ms | — | — |
| Machine 4 | worker, macOS | 8 / 98 ms | 28 ms | — | — |
| Machine 5 | worker, Linux | 58 / 234 ms | 116 ms | — | — |

Control daemon: 18–22 MB RSS, 0.8 % CPU, snapshot route 3.4 ms,
registry route 0.3 ms. Coverage: 2 611 samples, 0 dropped.

### What it shows

- **Typing on Machine 2 is distance.** Echo p50 116 ms ≈ the 117 ms link
  round trip. Only predictive echo can hide it (Link v2 follow-up).
- **Opening a terminal costs ~4 round trips.** Terminal setup on Machine 2
  is 469 ms ≈ 4 × 117 ms (identity proof + TCP + WebSocket upgrade), and the
  browser sees 802 ms before the first screen. This is the cost Link v2's
  persistent link removes (`docs/LINK_V2_DESIGN.md` §10).
- **Machine 5 has a variable path** (p50 58 ms, p95 234 ms) and the two
  Macs have long tails (p95 69–98 ms, max 108–307 ms) on otherwise ~6 ms
  links: worth a Diagnostics network test to see whether those are relayed
  periods or Wi-Fi.
- **Page load:** the snapshot step varied 95 ms–5.2 s over 3 loads while
  the snapshot route itself took 3 ms; the slow loads include the time spent
  on the sign-in screen, so this metric needs a sign-in mark before it is a
  useful baseline.
- **False "delayed" incidents (fixed after this report).** Every worker
  flipped between delayed and live every 20–30 s: a quiet worker uploads
  nothing, so its copy aged past the 10 s window between 30 s heartbeats.
  The control now refreshes a worker's sync time on a heartbeat that
  reports nothing pending and keeps such a worker live for 45 s; "delayed"
  after 10 s still applies when the worker reports pending changes.

## Still to do

- Collection overhead on and off (prompt §3.1).
- A 24 h report after every worker runs a release with daemon health
  reports, to fill in worker memory/CPU.
