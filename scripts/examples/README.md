# Sentinal manual test jobs

`long_job.py` is a deterministic, dependency-free process for testing Watch,
terminal handoff, interruption, failure, and saved-output replay.

Normal two-minute run:

```sh
python3 -u scripts/examples/long_job.py
```

Quick successful run:

```sh
python3 -u scripts/examples/long_job.py --steps 5 --interval 0.1
```

Intentional failure with exit status 3:

```sh
python3 -u scripts/examples/long_job.py --steps 10 --interval 0.5 --fail-at 4
```

The first lines include the PID, host, cwd, and configuration. Each step is
flushed immediately, so an attached terminal and the web UI see progress as
it happens. Ctrl-C prints an interruption message and exits with status 130.

## Production notification acceptance

These examples produce real Command and watcher Events through a normal FULL
Session. They never call a notification endpoint or edit SQLite, so they test
the complete production path from PTY output to Event, worker outbox, control
gateway, Web Push provider, and subscribed device.

Run one scenario on the Machine whose notification path you are testing:

```sh
bash scripts/run-notification-demo.sh success
bash scripts/run-notification-demo.sh failure
bash scripts/run-notification-demo.sh waiting
bash scripts/run-notification-demo.sh training
```

`success`, `failure`, and `waiting` use categories enabled by default. For the
waiting case, leave the prompt unanswered and detach with `Ctrl+J`; reattach
and answer `y` after the waiting notification arrives. `training` emits
recognised progress and writes one tiny checkpoint under `artifacts/` before
announcing it. Enable the Session's **checkpoint** and **progress** notification
rules if those extra notifications are under test; Command completion remains
enabled by default.

The scripts intentionally do not simulate resource pressure or a stalled
process. Those acceptance cases must come from real Metrics and elapsed time.
Remove the demo `artifacts/` directory after inspecting its checkpoint.
