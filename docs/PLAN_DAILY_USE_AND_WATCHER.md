# Plan: daily use and the watcher

Status: **proposal for owner review, 2026-11-05.** Comes from 3–4 weeks of real
use on five Machines (two VMs, one of them the control, and three devices).
Nothing here is built unless marked. Tracks are separate: each can ship without
the others, and each item is its own change, test-first, ending with `just check`.

Where this differs from `PLAN_V2_AI.md`, this plan wins:

- Rules and printed progress come before any model in the product.
- Models start small and swappable; fine-tuning waits for real collected data.
- The island gains a places menu (A2); Milestone 4.I's "no sidebar" stands.

## Track A: daily use

### A1. Remove Sessions

Today only a FAILED Session can be dismissed, and only from "Needs you". Nothing
can be deleted, so finished Sessions pile up.

- [x] **Hide:** any finished Session leaves every list on every device. Its
      history stays searchable. The Hidden Sessions page shows them, with Restore.
- [x] **Delete:** with confirmation, a finished Session's output, Commands,
      Events, metrics and search rows are erased. It disappears from the control
      too. Logged, and its IDs are never reused. A live Session must be stopped first.
- [x] **Clear finished:** one action on the Machine page that hides all of its
      finished Sessions, showing the count first. Delete all is on the Hidden page.
- [x] Available from the Session page and Needs you, on desktop and phone. The
      prompt can open the Hidden page (`hidden`); it has no hide or delete verb yet.

Contract change: `RESTORE_SESSION`, `DELETE_SESSION`, `SESSIONS_DELETED` and
tombstones from the worker (fixtures updated on purpose). A continued Session's
chain is hidden or deleted as one. A worker must be updated before it accepts
these; an older one answers with an error, which the UI shows.

### A2. Navigation you can see

The prompt and search stay: they are the fast path for someone who knows the
product. They must not be the only path for someone who does not.

- [x] The page name in the island drops down the main places: on hover on
      desktop, on a tap on the phone, with Enter from a keyboard. It takes no space
      until wanted. The owner tried a rail and a bottom bar and rejected them for
      the space they use (2026-11-05); the sheets stay the back navigation.
- [x] Teach the fast path: each place in the menu shows the prompt word for it.
- [x] Places: Overview, Terminals, Machines, History, Recipes, Settings.
- [ ] Check every page has a way back

### A3. UI pass

Session lists, terminal, phone layout, Machines and settings all need work. This
runs from a written list of specific problems from real use, not a redesign.

- [ ] Build the list with the owner, using `screenshot_issues/` and new captures
- [ ] Fix in priority order, each with a test where one is possible

## Track B: watcher

Recognise the work where possible, and give programs a simple way to say it
themselves when not. Both feed the same Events, so the UI and agents do not care
which one produced them.

### B1. Rules that recognise more

- [x] Explicit counters from any program (`task 3/50`, `[4/10]`, tqdm bars)
      (not committed yet)
- [ ] Metrics: a `METRIC_REPORTED` Event (loss, accuracy, tokens per second) from
      any program, not only Commands containing "train"
- [ ] Full-screen apps: rules read the parsed screen on a timer, because apps
      like nvtop and vim redraw rather than print lines
- [ ] Builds, test runs, downloads and servers without a dedicated adapter
- [ ] Long runs: rate, estimated finish, last-progress time, and a progress chart

### B2. Progress API for code

For a program being written now, whether by a person or a coding agent. It is one
printed line, so it needs no library and works in any language, in logs too:

```text
::sentinal progress current=3 total=50 unit=task
::sentinal metric name=loss value=0.81
```

- [ ] Parse these lines ahead of every guess; they are trusted over rules and
      models for that Session
- [ ] They stay untrusted output: they can only set progress and metrics, never
      run anything, change state, or send a notification
- [ ] Document them in the agent guide, with a copy-paste Python and shell example
- [ ] `sentinal help agents` as a second way to reach the guide
      (`sentinal agent-guide`, not committed yet)

### B3. Data collection

Needed before fine-tuning, so the model learns from real screens, not only
synthetic ones.

- [ ] Off by default, on per Machine, with a per-Session opt-out
- [ ] Sampled, not continuous: only when the screen changed, at most once a minute
      per Session, duplicates dropped, with a disk budget
- [ ] Secrets removed before anything is written. Redaction can miss things, so
      the owner reviews data before training
- [ ] Stored on that Machine, owner-only. Nothing leaves automatically
- [ ] Each sample keeps what the rules said, so disagreements and misses are easy
      to find and label
- [ ] `sentinal data export` writes the training and eval format (`eval/`)

### B4. Swappable models

- [ ] Optional step after the rules, off by default, set per Machine: server URL,
      model name, prompt format. It works with no model configured
- [ ] Any OpenAI-compatible local server, so a Mac mini can try bigger models
- [ ] A prompt format for untuned models. Today's formats (`functiongemma`,
      `compact`) only suit models trained on them
- [ ] Each worker reads its own screens; they never pass through the control
- [ ] Guards from `intelligence/`, re-checked in the daemon; results marked "inferred"
- [ ] Inferred results send no notifications
- [ ] Compare models on collected data with `eval/score.py`, so a choice rests on
      numbers

### B5. Fine-tune a small model

Starts only after B3 has enough labelled real data.

- [ ] Hand-labelled real-screen set as the decision metric
- [ ] Fine-tune, score against rules and the bigger models from B4
- [ ] It becomes the default only after passing the gate in `PLAN_V2_AI.md` §6.2

## Track C: multi-day runs survive

A daemon restart or update ends FULL Sessions, so it can end a multi-day run.

- [ ] Owner approves `docs/R1_KEEPER_DESIGN.md`
- [ ] Build keepers (R1 / 4.G P1). Until then, updating a Machine mid-run ends the run

## Track D: one install per Machine

From the 2026-11-05 incident: a test daemon reused this Mac's real link and replaced its
Sessions, address, and version on the production control.

- [x] `config_dir`: every credential and settings file follows one setting
- [x] Control asks for a full resend when a worker discarded changes it never received
- [x] Daemon install ID and build commit in heartbeats; the control refuses a second
      live install and tells the owner (`machine_instances`, migration 015)
- [x] `just dev`: `isolated` daemons cannot link, update, or use production's paths;
      a DEV badge; dev terminals reach the dev daemon (`docs/TESTING.md`)
- [x] `just promote`: `dev` reaches `main` only after every automated check

## Suggested order

1. A1 Remove Sessions
2. A2 Navigation
3. B2 Progress API
4. B1 Rules
5. C Keepers
6. B3 Data collection, so real data builds up as early as possible
7. A3 UI pass, alongside everything above
8. B4 Swappable models, then B5 fine-tuning

## Later

Agent tools above level 0, assistant features, and acting with approval stay in
`PLAN_V2_AI.md`, after this plan.
