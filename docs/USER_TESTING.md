# User testing the Sentinal interface

The simulation harness is a facilitator tool for testing navigation, wording,
state comprehension, and mobile layouts before every backend milestone exists.
It is not evidence that a future feature is implemented.

## Start it

```console
$ just user-test
```

Open `http://127.0.0.1:5173`. On first use, the mock setup screen accepts any
bootstrap text and a password of at least 12 characters. Then open
**Settings → Simulation**.

Normal development and network testing remain real-data paths:

```console
$ just network
```

There is deliberately no in-app control that can switch a real API build to
fixtures. Simulation Settings exist only when Vite started with
`VITE_DATA=mock`; every simulated screen carries a visible `MOCK` label.

## Scenarios

| Scenario | Use it to test |
|---|---|
| Mixed fleet | General navigation and prioritization across all fixture states |
| First run | Empty-state clarity and what the user tries to do first |
| Needs attention | Whether waiting, failure, and stalled work are distinguishable |
| Healthy training | Progress/resource comprehension and live change visibility |
| Capture limits | Whether users understand why adopted processes are read-only |
| Offline worker | Offline wording, retained state, and recovery expectations |

Playback can be paused, run every five seconds, run every second, or advanced
one deterministic tick. **Reset mixed demo** restores the committed baseline.
The scenario and playback setting live in `sessionStorage`; product data is
never written to the daemon or the real SQLite store.

## Suggested 20-minute study

Do not explain Sentinal's object model before the tasks. Ask the participant to
think aloud. Record time, first click, wrong turns, and their exact language.

1. **Orientation — Mixed fleet:** “Tell me what is running and what needs you.”
2. **Triage — Needs attention:** “Which Session would you inspect first, and
   what do you think happened?”
3. **Progress — Healthy training:** pause playback, ask what evidence says the
   run is healthy, then advance twice and ask what changed.
4. **Capture — Capture limits:** “Can you type into this Session? Why or why
   not? What would you do next time?”
5. **Connectivity — Offline worker:** “Is its work stopped, or is this browser
   disconnected? What do you expect when it returns?”
6. **Empty state — First run:** “What would you do to start monitoring work?”

After each task ask:

- What did you expect to happen?
- What wording or control was unclear?
- What information was missing for the decision?
- How confident are you, from 1–5?

## What this can and cannot validate

Useful now:

- information hierarchy, state/capture labels, navigation, responsive layout;
- destructive-action confirmations and comprehension of simulated changes;
- terminology and which evidence users need before acting.

Requires real API testing instead:

- terminal latency, Watch detach/reattach, shell behavior, Metrics accuracy;
- authentication security, pairing, multi-Machine routing, notifications;
- output replay, adopted-log tailing, watcher accuracy, daemon restart survival.

Never describe a successful simulated pairing, Recipe, notification, health,
or progress interaction as a working daemon capability. Record it as UI
validation for its owning roadmap milestone.
