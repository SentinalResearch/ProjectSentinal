# MILESTONE 4.F — Mobile web experience

Goal: on a phone, the installed Sentinal PWA is a first-class mobile app. It
has a bottom tab bar, native-feeling screens, readable Session rows, a usable
full-screen terminal, and confirmation sheets. It is built from the mobile
design handoff, so a push notification opens a screen designed for a phone
rather than a squeezed desktop page.

The reference is `sentinal-mobile-handoff/`:
- `MOBILE_SPEC.md` holds the rules;
- `Sentinal Mobile.dc.html` holds 17 screens at 390×844 (open it with
  `npx serve sentinal-mobile-handoff`).

Rebuild those screens as responsive layouts in the existing React + xterm.js
PWA. Desktop (≥ 768 px) keeps its current layout and behaviour.

Depends on completed Milestones 4.B, 4.C, and 4.D: the production PWA,
HttpOnly owner cookie, Web Push, and notification routing to exact namespaced
Sessions. This milestone changes the web client only.

Out of scope:
- a native or wrapper app, App Store work, Live Activities, or widgets;
- a protocol change unless the design review proves one is required;
- a new runtime dependency;
- any change to authentication, push, or routing security.

Before changing code, read `AGENTS.md`, `docs/ARCHITECTURE.md`,
`docs/SECURITY.md`, `docs/TESTING.md`, `docs/AUDIT_TRACKER.md`,
`docs/progress/milestone-4c.md`, `docs/progress/milestone-4d.md`,
`sentinal-mobile-handoff/MOBILE_SPEC.md`, the design file, and the current
`web/src` app shell, routes, styles, terminal, and key bar. Add Milestone 4.F
to `docs/ROADMAP.md` and create `docs/progress/milestone-4f.md` with the screen
checklist, proving tests, open questions, and the real-device matrix.

## Required design review — stop before tests or implementation

Present one concrete plan covering:

1. **Screen inventory.** Map each handoff screen to an existing route or
   component, or a new one. The screens are: A1 Landing, A2 Features,
   B1 Sign in, B2 Overview, B3 Session detail, B4 Waiting, B5 From a
   notification (failed), C1 Terminal, C2 Ctrl-C confirm, C3 Terminals,
   D1 History, D2 Filters sheet, D3 Recipes, D4 Run recipe, E1 More,
   E2 Machines, E3 Notifications, E4 Account. Show which desktop components
   are shared and which phone components are new.
2. **Breakpoint mechanism.** The mobile layout applies below 768 px. Decide CSS
   media queries versus one shared `useViewport` hook, and how a rotation or
   resize switches layouts without losing terminal state, scroll position, or
   unsent input. The current sidebar collapses to an icon rail below 1,200 px;
   phones must never see that rail (`MOBILE_SPEC.md` → Breakpoint).
3. **Navigation.**
   - The bottom tab bar has five tabs: Overview, Terminals, History, Recipes,
     More.
   - Detail screens get a 44 px back bar.
   - Map every existing desktop route (Overview, Session, Split, Project,
     History, Recipes, Settings tabs) to a tab or a detail screen.
   - Map every deep link (notification URLs, login return paths) to a phone
     screen.
   - Define how the browser back button and swipe-back behave.
4. **Data gaps.** List every value a phone screen shows that the client does
   not already have, then choose: derive it in the client, use an existing
   request, or omit or defer it. Known candidates:
   - C3 cards need the last two output lines of each live terminal;
   - B3 needs a "last output" age;
   - B5 needs "how it passed last time" (a previous successful run of the same
     Command);
   - E4 needs a list of signed-in browsers.

   Stop and ask before any protocol or daemon change. If one is unavoidable,
   show it as an additive change with fixtures and generated TypeScript.
5. **Sheets and confirmations.** One accessible bottom-sheet component for C2
   (Ctrl-C and Kill) and D2 (filters). Cover focus trap, Escape, scrim, drag
   handle, safe area, and screen-reader names. Nothing destructive may happen
   on a single tap.
6. **Terminal on a phone.**
   - xterm fills the space between the 52 px header and the key bar.
   - The key bar sits above the software keyboard (`visualViewport`).
   - Ctrl-C is red and asks for confirmation on long jobs.
   - Cover resize when the keyboard opens and closes, and landscape.
   - Read-only Sessions show no keys.
   - No terminal byte or credential is written to storage or a URL.
7. **Tokens and type.** The colour tokens already match `tokens.css`. Add a
   `--destructive` (#D12A20), the mobile type scale, and safe-area variables,
   without changing desktop appearance.
8. **Test approach.** Component tests at phone width, Playwright at 390×844,
   320×568, and 844×390 landscape against a production build, and how
   screenshots are compared with the design. Prefer reviewed screenshots
   stored as test artifacts over brittle pixel goldens.

Then stop for explicit human approval.

## 1. Shell and navigation

- Below 768 px, render the bottom tab bar instead of the sidebar. It is 83 px
  tall (49 + safe area), has a 1 px top border and five equal columns, and the
  active tab uses `--accent-fg` at weight 800.
- Every page uses 20 px side padding and honours
  `env(safe-area-inset-top/bottom)` in the installed standalone PWA.
- Back bars show "‹ Parent" in `--accent-fg`, with an optional ⋯ menu.
  Browser back and an in-app back reach the same place.
- The Machine filter becomes a horizontally scrolling chip row (it never
  wraps).
- Desktop (≥ 768 px) is unchanged; existing desktop tests keep passing.

## 2. The notification path (build first)

This is what a push notification opens, so it is accepted first.

- **B2 Overview:**
  - title with a "+" for a new Session;
  - Machine chip row;
  - "Needs you" heading with a count;
  - needs-you cards with a 3 px top line in the state colour; a waiting card
    shows its prompt in mono with 52×48 y/n buttons;
  - Machine groups of Session rows.
- **Session row (all phone lists):**
  - line 1: state badge and mono duration;
  - line 2: title 16/700;
  - line 3: mono Command in 12 px, one line with ellipsis.

  The badge never shares a line with the title; this fixes the overlap bug.
  Long Machine names stay on one line with ellipsis.
- **B3 Session detail:**
  - breadcrumb, title, badges (state, health, duration), Command;
  - big progress percentage and bar;
  - a 2×2 metric grid: LOSS (from the training summary), GPU, VRAM, last
    output;
  - health evidence and Events;
  - a sticky bottom bar: Open terminal (accent) and Stop (destructive
    outline, 96 px).
- **B4 Waiting:** the prompt large, in mono, on the terminal-dark block; 80 px
  y/n buttons that send input only where capture allows it; and Open terminal.
- **B5 Opened from a failure:**
  - red header "✕ FAILED · <Machine>" with a safe failure summary (never raw
    output lines that the notification policy forbids);
  - a three-cell stat grid;
  - last output, using the retained read-only view;
  - Open terminal, and the previous successful run if the design review found
    the data.
- **C1 Terminal:**
  - full-screen dark surface;
  - header with "‹ Detach", title, Machine/path, and badge;
  - key bar: Ctrl-C (red), Esc, Tab, ↑, ↓, y, n; 44 px keys that scroll
    sideways.
- **C2 Ctrl-C/Kill confirmation sheet:** grabber, title, explanation with the
  Command and how long it has run, destructive "Send Ctrl-C", and Cancel.
- Notification deep links land on B3, B4, or B5 according to the Session's
  state, keeping 4.C's rules for login return, missing Sessions, and offline
  or revoked Machines.

## 3. Lists and actions

- **C3 Terminals:** a stack of live terminal cards with a slim state header and
  the last two lines; tap opens C1; the focused card gets an accent outline.
  This replaces the desktop split view on phones.
- **D1 History:**
  - search field and a scrolling filter chip row, grouped by date;
  - rows: 28 px state square, mono Command with ellipsis, Machine · project ·
    duration, and a ⋯ menu (Save as Recipe, Open, Copy).
- **D2 Filters sheet:** wrapping chip groups (Machine, Project, Result, Date),
  Reset, and "Show N results".
- **D3 Recipes:** grouped by category; each row shows the name, the mono
  template with ellipsis, Machine · parameter count, and a chevron.
- **D4 Run Recipe:**
  - mono parameter fields with "default" hints, keeping today's validation and
    port-conflict warnings;
  - a target-Machine segmented control with offline Machines disabled;
  - command preview;
  - sticky "Run on <Machine>".

## 4. More and settings

- **E1 More:**
  - Machines with status, Projects, Settings rows;
  - Theme segmented control (Light / Dark / Auto);
  - the signed-in user, and Sign out as destructive text.
- **E2 Machines:**
  - Pair a Machine (the existing Add Machine flow);
  - Machine cards: name, CONTROL/WORKER tag, hardware, status;
  - Revoke, behind confirmation;
  - the Machine view from Milestone 4.B (system info, load graph and summary
    table, capture sources, processes) laid out for a phone.
- **E3 Notifications:**
  - the device push toggle card from 4.D;
  - per-Session rows with wrapping toggle chips (Finished, Error, Stalled,
    Waiting, Checkpoint, Progress) instead of the desktop checkbox table;
  - the same rules, rollback on refusal, and Machine scoping as today.
- **E4 Account:** change password, and signed-in browsers with Sign out, as the
  design review decided.

## 5. Public pages

- **A1 Landing:** 56 px header; left-aligned hero; stacked full-width buttons
  (Sign in in accent, Set up this server as an outline); a small native
  "needs you" preview, not a scaled desktop mock.
- **A2 Features:** stacked "01 / 04" sections with native rows; no tilted
  desktop screenshots on phones.
- **B1 Sign in:** 64 px logo, "Welcome back.", a 56 px password field with a
  Show toggle, and Sign in.

## Behaviour and quality rules

- Touch targets are at least 44×44; primary buttons are 52 px tall and full
  width.
- No border radius. Section headers have a 2 px `--text-1` rule; rows have a
  1 px `--line` rule.
- States: default; pressed (`bg-2`, buttons scale to 0.96); disabled
  (`--text-3`, no pointer); loading (label becomes "…" and the control is
  disabled).
- State colour is always paired with its glyph and label (● ? ‖ ✕ ✓ – ■ + ○),
  never colour alone. Badge text is always #0A0A0A.
- Motion only shows liveness (a subtle pulse on RUNNING) and respects
  `prefers-reduced-motion`.
- Fonts: Archivo for the UI, JetBrains Mono for Commands, paths, ports,
  prompts and durations; `font-variant-numeric: tabular-nums` throughout.
- Accessibility: logical focus order, visible focus, named controls,
  `aria-live` for errors and delivery state, and contrast that meets the
  existing conventions in both themes.
- No horizontal page scroll at 320 px except the deliberate chip rows and key
  bar.
- Performance: the Overview with 50 Sessions across 3 Machines scrolls
  smoothly on a mid-range phone. Terminal cards in C3 must not open one live
  WebSocket per off-screen card if the design review chooses a cheaper source.

## Tests first

- **Component tests at phone width** for:
  - the tab bar;
  - the Session row structure (badge never on the title line; long names
    truncate);
  - needs-you cards and the waiting y/n;
  - the bottom sheet (focus trap, Escape, no single-tap destructive action);
  - the metric grid with and without training data;
  - the E3 chips' rule behaviour and rollback;
  - D4 target selection with offline Machines disabled.
- **Routing:** every desktop route and notification deep link resolves to the
  intended phone screen. Back navigation returns to the parent. Same-numbered
  Sessions on two Machines never cross.
- **Playwright against a production build with two fake workers** at 390×844,
  320×568, and 844×390:
  - Overview → needs-you → waiting y/n;
  - a notification URL → login → B3/B5 → Open terminal → key bar → Ctrl-C
    sheet → cancel and confirm;
  - History search and filters → Save as Recipe;
  - Recipes → Run on Machine;
  - More → Notifications chips → Machines;
  - theme switch;
  - rotation without losing terminal state.

  Store a screenshot per screen as a reviewed artifact. Assert no horizontal
  overflow, no browser credential in storage or URLs, and no mock data.
- **Desktop regression:** the existing Vitest and Playwright suites pass
  unchanged at desktop widths.

## Real-device acceptance

Record literal results in `docs/progress/milestone-4f.md`, against the same
candidate commit as the 4.C/4.D real-phone run where possible:

1. Install the production PWA from the Tailnet HTTPS control on a real iPhone
   (Safari, added to the home screen). Add Android Chrome if one is available.
   Record OS and browser versions.
2. Walk every screen in both themes. Check safe areas, the notch and home
   indicator, standalone status bar, the keyboard with the key bar, and
   landscape.
3. Tap a real push from each worker. It opens B3, B4, or B5 for the exact
   Session.
4. Answer a waiting prompt with y/n.
5. Send Ctrl-C through the sheet.
6. Run a Recipe on the selected Machine.
7. Toggle a notification rule.
8. Record sanitized screenshots or a textual table per screen, and every known
   limitation. Do not claim a device or browser that was not tested.

## Done when

- All 17 handoff screens exist at phone width and match the spec's rules.
- The desktop layout is unchanged.
- `just check` and the production Playwright suite pass.
- The real-iPhone acceptance is recorded.
- The Session-row overlap bug is gone.
- No credential, terminal byte, or notification secret reaches browser
  storage, Cache Storage, URLs, or logs.

Report, stop, and wait for review.
