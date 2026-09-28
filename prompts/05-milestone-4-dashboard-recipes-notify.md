# MILESTONE 4 — Multi-machine dashboard, recipes, notifications (v0.4 = V1)

Goal: open the phone → see every machine → every session → progress and
resources → attach → launch a saved recipe → get notified when it finishes.

## 1. Multi-machine

- No central server. Web app keeps a list of machines
  `{ name, url (https Tailscale name), token }` and connects to each daemon
  directly. Offline machines show `OFFLINE` with last-seen time; sessions on
  them show `DISCONNECTED`.
- Document HTTPS via `tailscale serve --https=443 http://127.0.0.1:7420`
  in `docs/SETUP.md` (required for PWA + notifications).
- Daemon config gains `allowed_origins` for CORS; default empty.

## 2. Recipes

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

## 3. Notifications

- `notify.toml`: ntfy server URL + topic (+ optional token).
- Rules engine: default from `EventKind::notifies_by_default()`; per-session
  overrides `{ finished, error, stalled, waiting, checkpoint, progress }`.
- Dedup: same session + same kind at most once per 5 min, except
  `COMMAND_FINISHED`.
- Message format: `"<machine> · <session title or command>: <short text>"`.
  Never include raw output lines (may contain secrets); include event data only.

## 4. Mobile UI

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
- Web: machine list with one offline machine; recipe form validation;
  Playwright smoke test at phone viewport (390×844): list → attach → run recipe.

## Done when

The V1 demo from the spec works across two real machines (document steps),
`just check` passes. Report, stop. **This is V1.**
