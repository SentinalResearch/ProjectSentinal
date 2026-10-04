# Build prompt — Sentinal (give this to the coding agent)

You are building **Sentinal**, a local-first control plane for terminal sessions on a few personal machines. The design is final and lives in this folder. Read `README.md` fully before writing code, then open `design/Sentinal.dc.html` in a browser (serve the `design/` folder statically) and click through every screen.

## Stack
- React + TypeScript + Vite, installable **PWA** (manifest, service worker, offline shell).
- **xterm.js** for every terminal area (session detail, split view, phone terminal).
- Plain CSS with the variables in `tokens.css` (CSS modules or vanilla CSS, no CSS-in-JS runtime, no UI kit). Self-host Archivo and JetBrains Mono.
- Router: `/` (landing), `/login`, `/setup`, `/app/overview`, `/app/sessions/:id`, `/app/split`, `/app/projects/:id`, `/app/history`, `/app/recipes/:id?`, `/app/settings/:tab?`.

## Rules (non-negotiable)
1. Match the designs exactly: colors, type, spacing, 2 px / 1 px rules, square corners, motion timings and easing.
2. States are never color-only: always use `<StateTag>` (color + glyph + label).
3. Tabular numerals everywhere; units formatted as `21.4 GB`, `1h31m`, `96 %`.
4. Commands, paths, ports and prompts are monospace.
5. Destructive actions (Ctrl‑C, kill, revoke) always go through a confirm step; nothing destructive is one accidental click.
6. Terminal background is `--term-bg` in both themes; never theme program output.
7. Honour `prefers-reduced-motion`: disable all animation and show the final state.
8. Dark theme is the default; light theme must reach the same contrast (text ≥ 4.5:1, indicators ≥ 3:1).
9. Don't invent product features beyond the designs and the brief. If something is ambiguous, leave a `// QUESTION:` comment and list it in `QUESTIONS.md`.

## Build order
1. Tokens, fonts, global resets, `StateTag`, `SentinalMark`, buttons, inputs, rules.
2. Auth (all 5 modes plus the mark animation) and route guard.
3. App shell (sidebar, collapse, ⌘K) → Overview → Session detail (+ xterm, confirm) → Split view → History → Project → Recipes → Settings (+ pairing).
4. Landing page with scroll-driven animation (the page is its own scroll container; IntersectionObserver for reveals; rAF for the sticky showcase progress).
5. Responsive phone layouts from the same components (see README "Phone").

## What's real vs. mocked — IMPORTANT
Build **all UI now**, even where the backend doesn't exist yet. Anything not backed by a real API is built against a **mock adapter** so it can be swapped later without touching components.

- Create `src/data/adapter.ts` with one interface (`getMachines`, `getSessions`, `subscribe`, `sendInput`, `sendSignal`, `getHistory`, `runRecipe`, `pairMachine`, `rotateToken`, `revokeToken`, `auth.*`, `notifications.*`).
- `src/data/mock/` implements it from `fixtures.json`, with simulated latency (300–800 ms) and simulated live updates (epoch ticks, event appends, state transitions) so every screen looks alive.
- `src/data/api/` is the real implementation. Leave unimplemented methods throwing `NotImplemented`, and select the adapter with `VITE_DATA=mock|api`.
- Every mocked feature shows a small, unobtrusive **`MOCK` badge** (11 px mono, dashed `--line-strong` outline) next to its heading in dev builds only (`import.meta.env.DEV`).
- Keep a `FUTURE.md` table: feature · screen · status (`mock` / `partial` / `real`) · what's needed to make it real.

### Mock now, build for real later
These are in the brief but either not designed in full or not backed yet. Build **placeholder UI** that follows the design system, behind a feature flag (`src/flags.ts`, all `false` by default except in mock mode):
- **Phone screens (Round 2):** home, session detail, terminal with key bar, waiting-for-input, recipes, history, notification → failed pytest. Compose them from the desktop components; the specimens are in Foundations v2 §05.
- **Suggestion card:** "Training failed: CUDA out of memory. Previous successful run used batch size 16; this run used 32." The suggested command is monospace, with **[Open in terminal]** only. Never "Run"; the app never executes AI suggestions itself.
- **"What happened while I was gone?"**: a journal view grouping events since the last visit.
- **Ask box:** natural language → shows the exact action it will take and waits for confirmation. Never auto-executes.
- **Recipe editor / New recipe:** currently disabled in the design.
- **Push notifications:** Web Push subscription, with rules from Settings → Notifications.
- **Multi-browser session list** in Settings → Account (only "This browser" is shown now).

## Assumptions to confirm (from `fixtures.json → _assumptions`)
Pair command `sentinal-agent pair <TOKEN>`; token mask `sa_••••xxxx`; pairing TTL 10 min; login lock 3 attempts → 5 min; setup password ≥ 12 chars; `cargo build` on MacBook; relative history dates. Keep these in config, not hard-coded.

## Definition of done
- Every screen in `design/` is recreated and matches side by side at 1440 × 900 and 1024 × 768, in dark and light.
- The full click-through works on mock data: landing → login (wrong ×3 → locked) → setup → overview → answer y → session → Ctrl‑C confirm → split → history filters → recipe run → settings pairing.
- Lighthouse: PWA installable; accessibility ≥ 95.
- `FUTURE.md` and `QUESTIONS.md` are up to date.
