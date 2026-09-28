# MILESTONE 3 — Event engine and watchers (v0.3)

Goal: raw output + process state + metrics become Events. Everything above
this layer reads Events, never terminal text.

## Crate: `crates/watcher` (pure, no I/O)

```rust
pub struct Observation { /* at, kind: OutputLine(String) | ProcessSample(..) | MetricsSample(..) | CommandStarted(..) | CommandFinished(..) */ }

pub trait WatcherAdapter {
    fn name(&self) -> &'static str;
    fn matches(&self, command: &protocol::Command) -> bool;   // choose adapter by command
    fn observe(&mut self, obs: &Observation) -> Vec<protocol::EventKind>;
    fn state(&self) -> AdapterState;                          // structured current state
}
```

- Output lines: strip ANSI escapes, split on `\n` and `\r` (progress bars
  redraw with `\r`), cap line length at 8 KiB.
- **Rate limiting:** at most one `PROGRESS_UPDATED` per session per 5 s,
  unless percent moved ≥ 1 point.

## Adapters

1. **Generic** (always on):
   - `WAITING_FOR_INPUT` when the last line matches prompt patterns
     (`[y/N]`, `(yes/no)`, `Password:`, `Press any key`, ends with `? `) and no
     output for 2 s.
   - `POSSIBLY_STALLED` when no output for N minutes (default 10) **and**
     CPU < 5 % **and** GPU < 5 % for the session tree. Not when the GPU is
     busy (that's "probably computing").
   - `ERROR_DETECTED` / `WARNING_DETECTED` for `Traceback`, `panicked at`,
     `error:`, `CUDA out of memory`, `Killed`, `WARNING`.
2. **Training** (`python`/`torchrun`/`accelerate` + `train` in the command):
   epoch/step `x/y`, `loss=`, `val_loss`, `lr`, tqdm bars; checkpoint lines
   (`Saving checkpoint`, `.pt`/`.safetensors` paths) → `CHECKPOINT_CREATED`.
   State: epoch, total, last loss, best val loss, trend.
3. **pytest**: collected count, progress dots/percent, final
   `N passed, M failed` summary, failing test names.
4. **vLLM / llama.cpp server**: `Uvicorn running on`, `server is listening on`
   → `SERVER_STARTED { port, url }`; model name; OOM errors.
5. **cargo**: `Compiling x`, `error[E…]`, `test result:` summary.
6. **Declarative adapters:** TOML files in `~/.config/terminal-agent/adapters/`:
   ```toml
   name = "my-tool"
   match_command = "^mytool "
   [[rule]]
   regex = 'step (?P<current>\d+)/(?P<total>\d+)'
   emit = "PROGRESS_UPDATED"
   unit = "step"
   ```
   Invalid regex → clear error at load time naming file and rule.

## Health (deterministic, in watcher)

`Health { progress: Option<f32>, activity: Level, resources: Level,
stability: Level, errors: u32, overall: HEALTHY | WARNING | POSSIBLY_STALLED | FAILED, evidence: Vec<String> }`.
Rules only; every rating lists the evidence strings that produced it. Add
it to the protocol (fixture + TS).

## Daemon integration

Event engine task per session: feeds observations to generic + the matched
adapter, stores events, broadcasts `DaemonMessage::Event`. Web shows an
events timeline and a health card per session.

## Tests first

- Save **real logs** under `crates/watcher/tests/logs/<adapter>/*.log`
  (PyTorch Lightning, HF Trainer, plain tqdm, pytest pass/fail, vLLM,
  llama-server, cargo build error). Replay each through the adapter and
  snapshot the events with `insta`.
- Generic: stalled vs computing with synthetic metrics + fake clock;
  waiting-for-input positive and negative cases (a line merely containing
  "password" in text output must not trigger).
- `\r` progress bar redraws produce rate-limited progress events.
- Declarative adapter: valid file works; invalid regex error message.
- Health: one test per overall rating, asserting evidence.

## Done when

Starting a real training run shows epoch progress, checkpoints and health in
the UI without any AI. `just check` passes. Report, stop.
