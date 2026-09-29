# MILESTONE 4.B — Recipes, notifications, mobile V1 completion (v0.4)

Goal: from the completed multi-Machine dashboard, launch a safe saved Recipe
and receive event-driven notifications when work needs attention.

Depends on completed Milestone 4.0 secure enrollment and Milestone 4.A
aggregation. Do not store Machine tokens in Recipe or notification settings.

## 1. Recipes

- Files: `recipes/*.toml` (built-in) and `~/.config/terminal-agent/recipes/*.toml`
  (user). Format mirrors `protocol::Recipe` (+ optional `category`,
  `project`). Validate with `Recipe::validate()` at load; bad file → logged
  error naming the file, other recipes still load.
- Built-ins: llama.cpp server, llama.cpp bench, vLLM serve, HF model download,
  pytest, uvicorn/FastAPI, npm dev, cargo test, docker compose up.
- API: `LIST_RECIPES`, `RUN_RECIPE { recipe_id, values, session_id: Option }`
  (new session if none). Rendering always via `Recipe::render` (shell-quoted).
  Additive protocol change: fixtures + TS.
- "Save as recipe": from a succeeded command, prefill a recipe form; the user
  picks which literal values become parameters.

## 2. Notifications

- `notify.toml`: ntfy server URL + topic (+ optional token).
- Rules engine: default from `EventKind::notifies_by_default()`; per-session
  overrides `{ finished, error, stalled, waiting, checkpoint, progress }`.
- Dedup: same session + same kind at most once per 5 min, except
  `COMMAND_FINISHED`.
- Message format: `"<machine> · <session title or command>: <short text>"`.
  Never include raw output lines (may contain secrets); include event data only.

## 3. Mobile UI

Home: machines → sessions cards (state badge, progress bar, health, last
event). Session page: terminal, key bar, events timeline, health card,
Ctrl-C button with confirm. Recipes page with parameter form. Installable
PWA (manifest, icons, service worker for the app shell only).

## Tests first

- Recipe loading: valid, invalid TOML, undeclared placeholder, duplicate id
  (user file overrides built-in, with a log line).
- `RUN_RECIPE` with an injection value produces a quoted command (assert
  exact bytes written to the fake PTY).
- Notification rules: defaults, overrides, dedup window with fake clock;
  ntfy client tested against a local fake HTTP server.
- Web: Recipe form validation, per-Machine launch target, and notification
  settings; Playwright smoke test at phone viewport (390×844): Machine list →
  attach → run Recipe on the selected Machine.

## Done when

The V1 demo from the spec works across two enrolled real Machines (document
steps), `just check` passes, and no Recipe or notification crosses Machine
credentials. Report, stop. **This is V1.**
