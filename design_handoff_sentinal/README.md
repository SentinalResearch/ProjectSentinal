# Handoff: Sentinal — web app, public site and design system

> **Repository status:** this directory preserves the original design handoff.
> The implemented React application under `web/`, its tests, and the current
> protocol are authoritative where they differ. The handoff remains useful for
> visual intent and assets; it is not a statement of current feature status.

## Overview
Sentinal is a local-first control plane for terminal sessions across a few personal machines (Linux RTX-PC, Mac Mini, MacBook). One technical user starts long jobs (training, LLM servers, test suites), walks away, and checks them later from a browser or phone. The UI answers four questions at a glance:

1. What am I running? 2. How is it going? 3. Can I reach it and act on it? 4. How did I run this last time?

It is **not an AI terminal**. It is a calm dashboard over structured state, with a real terminal one click away. Target stack from the brief: **React + xterm.js, installable PWA**, plain HTML/CSS.

## About the design files
Everything in `design/` is a **design reference built in HTML**: working prototypes that show the intended look and behaviour. It is **not production code to copy**. Recreate these designs in the target codebase (React + xterm.js per the brief) using its own patterns. If no codebase exists yet, scaffold React + Vite + TypeScript and implement there.

To view the prototypes, open `design/Sentinal.dc.html` in a browser from a local static server (e.g. `npx serve design`). `support.js` is the prototype runtime only; don't port it.

## Fidelity
**High fidelity.** Colors, type, spacing, rules, states and motion are final. Recreate them exactly. Data in the prototypes comes from the brief (see `fixtures.json`). Values marked as assumptions in `fixtures.json → _assumptions` were invented and need product confirmation.

## Files
| File | What it is |
|---|---|
| `design/Sentinal.dc.html` | Root router: landing → auth → app. Holds theme tokens, keyframes, favicon. Persists route in `localStorage['sentinal.proto']`. |
| `design/SentinalLanding.dc.html` | Public marketing page with scroll-driven animation. |
| `design/SentinalAuth.dc.html` | Login, first-run setup, wrong password, locked, session expired. |
| `design/SentinalApp.dc.html` | App shell: collapsible sidebar + main area router. |
| `design/SentinalOverview.dc.html` | Overview: Needs you + sessions grouped by machine. |
| `design/SentinalSession.dc.html` | Session detail, embedded terminal, Ctrl‑C confirm dialog. |
| `design/SentinalSplit.dc.html` | 2–4 terminals side by side. |
| `design/SentinalProject.dc.html` | Project page (TIM, MentalAnimAItion). |
| `design/SentinalHistory.dc.html` | History search with filters. |
| `design/SentinalRecipes.dc.html` | Recipe list + run form. |
| `design/SentinalSettings.dc.html` | Machines & tokens (+ pairing flow), Notifications, Account. |
| `design/StateTag.dc.html` | State tag component (9 states). |
| `design/SentinalMark.dc.html` | Logo component. Final mark = variant `g` (slash + dot). |
| `design/Sentinal Logo.dc.html` | Logo exploration board (2c chosen). |
| `design/Round 1 Foundations v2.dc.html` | Foundations sheet: tokens, type, grid, component specs, phone specimens. |
| `tokens.css` | Paste-ready CSS variables, dark + light. |
| `fixtures.json` | All mock data used in the designs. |
| `assets/*.svg` | Logo mark (on dark, on light, mono) and app/favicon icon. |
| `AGENT_PROMPT.md` | The build prompt to give to coding agents. |

## Design system

### Principles
- Swiss / International style, toned down: strict grid, flush-left type, hierarchy from size and weight, **square corners everywhere**, rules instead of cards, one accent.
- Dark theme default; light theme must work. The terminal area is always neutral `#0C0C0C` in both themes.
- **State is never color-only**: every state has a color, a glyph and a label.
- Numbers use tabular figures (`font-variant-numeric: tabular-nums` on the root) and fixed units: `21.4 GB`, `1h31m`, `96 %` (space before %).
- Commands, paths, ports, prompts, event types and tokens are always monospace.
- Motion only to show liveness or a state change. Respect `prefers-reduced-motion` (all animation off, final state shown).

### Tokens
See `tokens.css`. Key points:
- Surfaces: `--bg-0` page, `--bg-1` sidebar / insets / sheets, `--bg-2` tracks, pressed, hover.
- Rules: 2 px `--text-1` starts a section or table; 1 px `--line` between rows; `--line-strong` for control outlines (≥ 3:1).
- Accent `--accent` (ultramarine) for primary buttons, the active pane and the logo dot only. `--accent-fg` for links, back links and the focus ring.
- State tokens come in pairs: `--s-<state>` is the **solid fill** (tag background, with `--ink` glyph and label on top). `--s-<state>-fg` is used for dots, bars and text drawn on a surface. Fills are identical in both themes; `-fg` is darker in light theme for contrast.

### Type
Archivo (UI) + JetBrains Mono (code). Google Fonts in the prototype; self-host for the PWA.

| Role | Size / line-height / weight | Letter-spacing |
|---|---|---|
| Display (percent) | 88/72 · 800 (session), 48–56 in cards | −0.06em |
| Page title | 36/38 · 800 | −0.03em |
| Session title | 40/42 · 800 | −0.035em |
| Section heading | 16–18 · 800 | 0 |
| Body | 14/20 · 400 (15/22 on phone) | 0 |
| Label | 13/18 · 500–600 | 0 |
| Overline | 11/14 · 700, uppercase | 0.08em |
| Mono | 12–13.5 · 400 (terminal 13/19) | 0 |

### Spacing and layout
- 4 pt scale: 4, 8, 12, 16, 20, 24, 32, 40, 48.
- Desktop content padding 40–48 px; content max-width 1200 px (session 1320).
- Sidebar: 264 px expanded, 57 px collapsed. It auto-collapses under 1200 px viewport width.
- Controls: buttons 36 px (desktop) / 44–48 px (phone and primary CTAs); inputs 44–48 px; rows ≥ 52–60 px; touch targets ≥ 44 × 44.

### Components (see Foundations v2 §04 for specimens)
- **StateTag**: solid fill + black glyph + UPPERCASE label, 700 weight, 0.04em. Sizes: md 26 px / 12 px text, sm 20 px / 10.5 px. `line` variant for dense tables; `iconOnly` where the label is in an adjacent column. DISCONNECTED is outlined (dashed ring + slash). RUNNING glyph pulses every 2 s.
  Glyphs: created `+`, idle `–`, running filled dot, waiting `?`, stalled `‖`, completed `✓`, failed `✕`, interrupted filled square, disconnected dashed circle with a slash.
- **Machine chip**: a column in a 3-up grid under a 2 px rule. 7–8 px square status dot (online = `--s-running`; offline = dashed outline, row at 55 % opacity).
- **Session row**: tag · name (700) + mono command (`--text-3`) · fact · project · duration; hairline below; the whole row is clickable; hover `--bg-1`.
- **Progress**: known total = 6–8 px bar in `--s-running-fg` with 10 % tick cuts; unknown total = 20 % segment sliding (1.8 s) only while output arrives; no activity = static diagonal hatch in stalled color.
- **Resource meter**: big value + small unit, 4 px neutral (`--text-1`) bar, 2 px threshold tick at 95 %. Over the threshold the meter becomes a solid `--s-waiting` field with ink text and ▲ HIGH.
- **Health card**: header field filled with the overall state color (ink text), 6 fixed rows (Progress, Activity, Resources, Stability, Errors, Overall), evidence line.
- **Event row**: time (mono, `--text-3`) · 8 px color square · TYPE (mono 11 px 700) · detail (mono). Info = accent, warning = waiting, error = failed, routine = `--line-strong`.
- **Buttons**: primary = accent fill; secondary = 1 px `--line-strong` outline (hover `--text-1`); destructive = `--s-failed` fill with ink text, **only inside a confirm dialog**. Destructive entry points use an outlined red "Ctrl‑C…" (the ellipsis signals the confirm step).
- **Confirm dialog**: scrim + `--bg-1` panel, 4 px red inset top rule, overline "INTERRUPT · MACHINE", 26 px question, consequence sentence, Cancel | Send Ctrl‑C at equal width. Esc or a click on the scrim cancels.

## Screens

### Public
**Landing** (`SentinalLanding`): its own scroll container (`height:100vh; overflow-y:auto`), sticky 60 px frosted nav (`backdrop-filter: saturate(1.4) blur(14px)`, background = bg-0 at 78 %).
1. Hero (100vh): 120 px animated mark → 3-line headline `clamp(44px,7.2vw,104px)`, 800, −0.05em, third line `--text-3` → subtext → CTAs → scroll cue.
2. Showcase (420vh, sticky inner): an app mock goes from `translateY(60px) scale(.82) rotateX(24deg)` to flat over the first 22 % of scroll, then steps through the 4 questions. The current area is at full opacity, the rest at 0.18.
3. Progress: a 68 % counter (ease-out-quart, 1.6 s) with bar and metrics.
4. Phone: device mock rises in → a notification drops at +1.4 s → y is tapped at +3.0 s → npm row goes WAITING → RUNNING and the Needs you count goes 3 → 2.
5. Recipes (black section): the template, the rendered command and "Serving on :8000" type out at 28 ms per character.
6. States: the 9 tags pop in with a 60 ms stagger.
7. CTA: "Walk away. Sentinal is watching." Footer.

Reveal trigger: an element's top < 70–75 % of the viewport. Each reveal runs once. Easing `cubic-bezier(.2,.7,.2,1)`, 0.9–1.1 s.

**Auth** (`SentinalAuth`): one centred block (max 880 px). Left: animated mark (88 px), typed overline `> sign in` + typed title with a blinking accent block cursor (45 ms per character), description fades in afterwards. Right (1 px left rule): form. Faint dot grid behind (24 px, radial mask).
- Modes: `login`, `wrong` (red field border, "Incorrect password. N attempts left before a 5-minute lock."), `locked` (disabled field, mm:ss countdown, red dot, no pulse), `expired` (yellow dot, "Returning to Overview"), `setup` (new + confirm fields, live ✓ rules: ≥ 12 chars, both match).
- Mark states: idle = blue dot with a 2.4 s ping ring (server reachable); checking = a segment runs along the slash (0.8 s loop), button says "Checking…"; success = the dot scales ×140 over 0.55 s, then route to the app; failure = shake 0.42 s and the dot turns red.

### App (`SentinalApp` shell)
Sidebar: mark + wordmark, collapse toggle, **New session** (→ Recipes), **Search history ⌘K** (→ History; ⌘K / Ctrl‑K works globally), nav (Overview with a yellow count badge, Split view, History, Recipes), MACHINES (each filters the Overview), PROJECTS, then Settings, theme toggle, avatar + Sign out.

- **Overview**: title + summary line. **Needs you** grid (WAITING with inline y/n, FAILED, STALLED); answering y/n removes the item. Then one table per machine (2 px rule header: name, status dot, hardware). Offline machine at 55 % opacity with a "last seen" line.
- **Session detail**: breadcrumb, 40 px title + tag + health outline tag, mono command, meta. Actions: Open in split view, Ctrl‑C…. The top block changes with session kind:
  - training: big percent + ticks + LOSS / GPU / VRAM
  - failed: red panel "3 failed", 182 passed · 42s, Run again / Show output
  - waiting: 40 px mono prompt + 88 × 72 y/n keys
  - stalled: "No output 14m", 78 % confidence
  - server: `:8000`
  - interrupted: notice

  Below: terminal (440 px, attached indicator, input line, key strip Ctrl‑C Esc Tab ↑ ↓) + side column (health card, events newest first).
- **Split view**: header with layout 2 / 3 / 4 and "+ Add terminal" menu. Panes are separated by 2 px `--line` gaps. Each has a 36 px header (tag, name, machine · fact, Details, ×). The active pane has a 2 px accent inset top and a blinking cursor. With 3 panes, the first spans 2 rows.
- **Project**: 56 px title + path; Sessions, Command history (→ History filtered), Event journal (with the session name next to each event).
- **History**: 56 px search bar with a 1.5 px `--text-1` border, autofocused; filter chips (Machine, Project, Result, Date) that invert when selected; table with a min width of 820 px that scrolls horizontally in its container; empty state with Reset filters.
- **Recipes**: 280 px list (category SERVERS) + run form: parameters with default and Reset, validation (port 1–65535, context a positive integer, model required), a warning when :8000 is already used on RTX-PC, target machine picker (offline machine disabled), live command preview in a terminal block, "Run on RTX-PC" → Starting… → opens the new session.
- **Settings**:
  - Machines & tokens: table with Rotate and Revoke… (inline confirm).
  - Pair new machine, 3 steps: name → token + command with a 10:00 TTL countdown, Copy, waiting bar → connected.
  - Notifications: a matrix of sessions × finished / error / stalled / checkpoint / progress, with a default row and a preview of both example notifications.
  - Account: change password (≥ 12 chars, match), signed-in browsers, Sign out.

### Phone (reference only)
The handoff's phone specimens are in Foundations v2 §05 and the landing phone
section: home glance, running training, terminal with key bar
`Ctrl‑C Esc Tab ↑ ↓ y n` (48 pt keys, Ctrl‑C red, y/n inverted), and a
bottom-sheet confirm. The original handoff did not design every phone screen;
the responsive implementation and 390×844 acceptance tests now live in `web/`.

## State management (suggested)
- `auth`: `{ status: 'setup'|'signedOut'|'signedIn'|'expired'|'locked', attempts, lockUntil }`
- `theme`: `'dark'|'light'`, persisted.
- `machines[]`, `sessions[]` (with `state`, `progress`, `metrics`, `health`, `events[]`, `prompt?`), `history[]`, `recipes[]`, `notificationRules{sessionId:{finished,error,stalled,checkpoint,progress}}`.
- Live updates over a WebSocket per server; terminals over xterm.js attach streams.
- UI-only state: sidebar collapsed (auto under 1200 px), split panes `{ids[], active}`, history filters, confirm dialog target.

## Assets
- Logo: `assets/sentinal-mark-*.svg` (a heavy slash from (5,22) to (13.5,2), stroke 3.4, square caps + a dot r 3.4 at (18,18.5), viewBox 24). Ink plus an accent dot; the mono version uses one color.
- App icon / favicon: `assets/sentinal-app-icon.svg`.
- No photos or illustrations. No emoji.
