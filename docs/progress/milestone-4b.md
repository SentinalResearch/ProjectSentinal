# Milestone 4.B progress — Recipes, notifications, mobile dashboard (V1)

Status: **implementation and isolated acceptance complete on 2026-10-04;
the two-real-Machine V1 demo is pending.** The roadmap's final box stays open
until the run below is appended with its literal output.

Milestone implementation acceptance below was recorded at commit `9cb5a95`;
the audit-hardening regression checks are recorded in `docs/AUDIT_TRACKER.md`.

## Delivered

### Recipes

- Built-in Recipes live in `recipes/*.toml` and are embedded in the daemon:
  llama.cpp server, llama.cpp bench, vLLM serve, HF download, pytest,
  uvicorn, npm dev, cargo test, and docker compose up.
- User Recipes are read from `~/.config/terminal-agent/recipes/*.toml`. Each
  file is checked with `Recipe::validate()` at load. A bad file is logged with
  its path and skipped, and the other Recipes still load. A user Recipe with a
  built-in's ID replaces it, and the override is logged.
- Templates use `{name}` for parameters and `{{` / `}}` for literal braces.
  A placeholder must be a standalone, unquoted shell word; quoted or adjacent
  placeholders are rejected because wrapping a value cannot isolate shell
  syntax when the surrounding template already controls its quote context.
  `RUN_RECIPE { recipe_id, values, session_id }` renders through
  `Recipe::render`, which shell-quotes every value. The daemon test asserts the
  exact bytes written to the fake PTY for an injection-shaped value.
- `LIST_RECIPES`, `RUN_RECIPE` and `SAVE_RECIPE` are additive protocol changes:
  `PROTOCOL_VERSION` stays 1, and the fixtures and generated TS were updated.
- **Save as Recipe** opens from a succeeded Command. The owner ticks the
  literals that become `{valueN}` parameters. Every other fragment keeps its
  braces literal (`awk '{print $1}'` is saved as `awk '{{print $1}}'`). The
  file is written only on the Machine that ran the Command, and a refusal is
  shown inline.
- Each Machine serves its own catalog. The web data layer gives each entry the
  ID `<machine-id>/<recipe-id>` and sends `RUN_RECIPE` only to that Machine.
  The catalog route encodes this ID as a single path segment.

### Notifications

- Each Machine reads its own `~/.config/terminal-agent/notify.toml`:

  ```toml
  server = "https://ntfy.sh"         # HTTPS; plain HTTP only to loopback or a Tailscale 100.64.0.0/10 address
  topic  = "sentinal-<random>"       # letters, digits, - and _
  token  = "tk_..."                  # optional, sent as a Bearer token, never returned to a browser
  ```

- Rules:
  - The default `NotificationRuleSet` notifies on finished, error, stalled
    and waiting, and leaves checkpoint and progress off. A unit test proves it
    agrees with `EventKind::notifies_by_default()`, including the 95 % resource
    threshold.
  - The owner can change each Machine's default and set per-Session overrides,
    which are stored in `data_dir/notification-rules.json`.
- Deduplication allows the same Session and kind at most once per 5 minutes,
  except `COMMAND_FINISHED`. It is tested with a fake clock.
- Capture honesty: an Event the Session's capture level cannot support as
  evidence (`CaptureLevel::supports_evidence`) never notifies, whatever the
  rules say.
- Text is `"<machine> · <session title or command>: <short text>"`, built
  from event data only. `ERROR_DETECTED` sends `error detected` and never the
  matched output line.
- The Settings → Notifications table shows what each daemon actually applies:
  the Session override, otherwise that Machine's default. A refused change
  rolls back and is reported.

### Mobile dashboard and PWA

- Home lists Machines and their Session cards. The Session page has the
  terminal, key bar, Events, health and Ctrl-C with confirmation. The Recipes
  page has a parameter form, port-conflict warnings and a per-Machine target.
- `manifest.webmanifest` includes any-purpose and maskable 192/512 icons. The
  service worker caches the app shell only and never handles `/api/`.

### Credential isolation

- Recipe files and `notification-rules.json` hold no Machine token, link
  credential or pairing code. The ntfy token stays in `notify.toml` on its
  Machine, and `NotificationStatus` reports only `token_configured`.
- `RUN_RECIPE`, `SAVE_RECIPE` and rule changes go through the
  control-to-worker route for the owning Machine, the same path that 4.A
  proved for input. Browser storage still holds only `sentinal.sessionToken`
  and the theme, as `web/e2e/enrollment.spec.ts` asserts.

## Automated acceptance record

```console
$ cargo test -p daemon --lib notifications      # defaults, overrides, dedup, capture gating,
7 passed                                        # raw-line exclusion, HTTP scheme, fake ntfy server
$ cargo test -p daemon --test recipes           # exact PTY bytes for an injection-shaped value
$ pnpm -C web check
30 files, 140 tests passed; TypeScript passed
$ pnpm -C web test:e2e                          # 390×844: Machine list → attach → run Recipe
5 passed                                        # asserts the new Session belongs to the catalog's Machine
$ just check
passed (fmt, clippy -D warnings, workspace tests, bindings, web, shell checks)
```

## V1 demo — two enrolled real Machines

**NOT RUN yet.** The workers need a release that contains 4.B. Stable 0.1.6
predates it.

1. Publish the 4.B worker build from the control
   (`scripts/build-worker-release.sh`, then
   `scripts/publish-worker-release.sh publish`), then run `sentinal update` on
   both workers. Record `sentinal --version` on each.
2. On each worker, write `notify.toml` (above) for the same ntfy topic, and
   subscribe to that topic on the phone. Restart the worker daemon.
3. On the phone, open the control origin
   (`https://<control>.<tailnet>.ts.net`), sign in, and *Add to Home Screen*.
   Launch it from the icon and confirm it opens standalone.
4. Home shows both Machines Online with their Sessions. Attach to one Session
   on each Machine and type into it. The other Machine's terminal must not
   change.
5. Recipes: pick **cargo test** (or **pytest**) from Worker A's catalog and
   run it. The new Session opens under Worker A only. Repeat on Worker B.
6. When the Command finishes, the phone receives
   `<Worker A> · cargo test: command finished with exit 0`. Start a
   `read -p "Continue? "` Session and confirm one *waiting for input* push,
   not a repeat within 5 minutes.
7. From the finished Command, choose **Save as Recipe**, make one literal a
   parameter, and save. The Recipe appears only in that Machine's catalog
   (`~/.config/terminal-agent/recipes/<id>.toml` on that host) and runs again.
8. Check that no Machine token, link credential or pairing code appears in:
   - any `recipes/*.toml`, `notification-rules.json` or ntfy message;
   - the browser storage.
   Commands: `grep -r` on each host for the token-file contents; DevTools →
   Application.

Append the literal command output, both OS versions and Tailscale IPs, and
the tested commit below, then tick the final roadmap box.
