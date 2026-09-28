# MILESTONE 2 — Command tracking, history, metrics (v0.2)

Goal: a history view like

```
✓ cargo build            31s
✗ pytest                 42s
● python train.py        running   GPU 96%  VRAM 21.4 GB
```

## 1. Shell integration (`shell/`)

- `ta.zsh` (`preexec`/`precmd`), `ta.bash` (vendored `bash-preexec`),
  `ta.fish` (`fish_preexec`/`fish_postexec`).
- Emit OSC 133: `A` prompt start, `B` command start, `C` output start,
  `D;<exit>` command end. Emit `OSC 7` for cwd (`file://host/path`) and a
  custom `OSC 7420;cmd=<base64 command text>` so the exact command line is
  known without parsing the prompt.
- The daemon auto-sources the hook for the shell it spawns (via `ZDOTDIR`
  wrapper / `--rcfile` / `fish -C`), then the user's normal rc files.
  Sessions still work if the hook fails (state just stays IDLE).

## 2. OSC parser (in `crates/daemon/src/osc.rs`)

A streaming parser that extracts these sequences from PTY bytes **split at
any byte boundary**, returns `ShellSignal` values, and passes all bytes
through unchanged to the terminal. Unknown OSC codes pass through untouched.

## 3. Command tracker

`ShellSignal` → `Command::start` / `Command::finish`, session state
`IDLE ↔ RUNNING`, events `COMMAND_STARTED` / `COMMAND_FINISHED`. Commands
that start and end without output (e.g. `cd`) still count.

## 4. Store (`crates/store`)

- `rusqlite` (bundled), WAL mode, migrations as numbered `.sql` files run
  in order with a `schema_version` table.
- Tables: `machines`, `sessions`, `commands` (+ FTS5 virtual table on `text`),
  `events`, `metrics_raw`, `metrics_minute`.
- API: `insert/update session`, `insert_command`, `finish_command`,
  `insert_event`, `history(session: Option, limit)`, `search_commands(query, limit)`,
  `events_since(event_id)`.
- Retention: raw metrics kept 1 h, then downsampled into 1-minute averages
  by a function you can call with an injected "now".
- Daemon restarts: sessions whose PTY no longer exists are marked
  `INTERRUPTED` on startup, with an event.

## 5. Metrics

- `sysinfo` sampler every 2 s → `MachineMetrics`; per-session process tree via
  `Process` + `tree_totals` → `SessionMetrics`.
- NVIDIA via `nvml-wrapper` behind cargo feature `nvidia`; absent GPU is
  not an error. macOS GPU: `None` for now.
- Emit `RESOURCE_HIGH` when a value stays ≥ 90 % for 3 samples in a row
  (debounced, once per crossing).
- Stream metrics to clients subscribed to a session.

## 6. Web

History panel (✓ ✗ ●, duration, cwd), search box (FTS), live CPU/RAM/GPU
bars per session.

## Tests first

- OSC parser: every sequence; sequence split at every possible byte offset
  (loop over offsets); unknown OSC passes through; ordinary text untouched;
  garbage/unterminated OSC doesn't eat the rest of the output forever
  (limit length, e.g. 4 KiB).
- Tracker: `B`…`D;0` → succeeded command with correct duration (fake clock);
  `D;130` → failed; two commands in a row; `D` without `B` ignored with a
  warning.
- Store: migrations from empty DB; FTS finds `train` in `python train.py`;
  downsampling produces correct averages; restart marks orphans INTERRUPTED.
- Metrics: debounce logic with synthetic samples.
- Integration (`#[ignore]`): real zsh, bash and fish (skip if not installed):
  run `true`, `false`, `cd /tmp` and check stored exit codes and cwd.

## Done when

History, search and live metrics work in the web UI, `just check` passes,
report sent. Stop.
