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
