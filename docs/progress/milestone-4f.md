# Milestone 4.F progress — Mobile web experience

Status: **APPROVED as proposed by the owner on 2026-10-08** — Ctrl-C confirms
after 60 s of running (Kill and Revoke always), C3 uses read-only peek attach,
Overview "+" creates a FULL Session on the control, and B3 shows "LAST ACTIVITY".
Implementation complete and automated evidence recorded below; real-iPhone
acceptance is pending (owner's device). **Revised 2026-10-12** for handoff v2:
the tab bar is replaced by a top bar, a title switcher, and one Menu with
search (see "Revision 2" below, which supersedes the tab-bar parts of Design).

Reference: `sentinal-mobile-handoff/` (`MOBILE_SPEC.md` and `Sentinal Mobile.dc.html`,
17 screens at 390×844). Prompt: `prompts/05f-milestone-4f-mobile-web.md`.

## What exists today (inspected)

| Area | Current behaviour | Consequence |
|---|---|---|
| Shell | `AppShell` collapses `Sidebar` to an icon rail below 1,200 px; there is no bottom navigation | Phones see the icon rail the spec forbids |
| Phone CSS | Small `@media (max-width: 640px)` blocks in about 15 modules; no shared breakpoint | Partial reflow only, no phone structure |
| Routes | `/app/overview`, `/app/sessions/:id`, `/app/split`, `/app/projects/:id`, `/app/history`, `/app/recipes[/:id]`, `/app/settings[/:tab]` | All reusable; two phone routes are needed (below) |
| Notification click | `sw.js` validates the route with `safeNotificationRoute` and opens `/app/sessions/<machine>:<id>`; `RequireAuth` keeps the return path through `safeReturnPath` | Deep links already exist; phone screens must render them |
| Terminal | `TerminalPane` (xterm) and `keybar.tsx` (key sequences); `ConfirmDialog` is a centred modal | The key bar and dialog logic can be reused; phone layout and sheet are new |
| Tokens | `tokens.css` colours equal the handoff in both themes; no `--destructive`; no safe-area variables | Additive token work only |
| Data | Session `updatedAt`, Events, health evidence, training summary, `result` (failed/passed) for test runs, history search, Machine status | Enough for every screen except a true "last output" age (see Data gaps) |

## Design

### 1. Screen inventory

| Screen | Route | Reuses | New phone component |
|---|---|---|---|
| A1 Landing | `/` | `LandingPage` content and copy | `PhoneLanding` (header, hero, stacked buttons, native needs-you preview) |
| A2 Features | `/` (below A1) | landing sections, `LandingProgress` data | stacked "01 / 04" sections |
| B1 Sign in | `/login` | `LoginPage` logic, lockout, show toggle | phone layout only (CSS) |
| B2 Overview | `/app/overview` | `useLiveData`, `needsAttention`, `formatSession*`, `StateTag` | `MachineChips`, `NeedsYouCard`, `PhoneSessionRow` |
| B3 Session detail | `/app/sessions/:id` when not waiting/failed | `SessionTopBlock` data, `HealthCard`, `EventsList`, `ResourceBars` values, `lossOf` | `PhoneSessionDetail`, `MetricGrid`, `StickyActions` |
| B4 Waiting | `/app/sessions/:id` when `WAITING` with a prompt | capture capabilities, `sendInput` | `WaitingPanel` (80 px y/n) |
| B5 Failed | `/app/sessions/:id` when `FAILED` | `session.result`, retained read-only output, history | `FailurePanel`, `StatGrid` |
| C1 Terminal | `/app/sessions/:id/terminal` (new) | `TerminalPane`, `keybar` sequences | `PhoneTerminal` (header, viewport-aware key bar) |
| C2 Ctrl-C / Kill | sheet over C1/B3 | `sendSignal` | `BottomSheet`, `ConfirmSignalSheet` |
| C3 Terminals | `/app/split` on phones | `SplitViewPage` Session selection | `TerminalCard` stack |
| D1 History | `/app/history` | `HistoryPage` data, search, Save as Recipe | `PhoneHistoryRow`, filter chip row |
| D2 Filters | sheet over D1 | `HistoryFilters` | `BottomSheet` with chip groups |
| D3 Recipes | `/app/recipes` | `getRecipes`, categories | `PhoneRecipeRow` |
| D4 Run Recipe | `/app/recipes/:id` | `RecipesPage` validation, port warnings, `runRecipe` | `SegmentedControl`, sticky Run bar |
| E1 More → Settings (rev. 2) | `/app/more` → `/app/settings` | theme context, auth logout | `MorePage` → `PhoneSettingsHome` |
| E2 Machines | `/app/settings/machines` | `MachinesTab` actions, `AddMachineDialog`, `MachinePanel` | `PhoneMachineCard` |
| E3 Notifications | `/app/settings/notifications` | `NotificationsTab` rules, rollback, 4.D device card | `RuleChips` |
| E4 Account | `/app/settings/account` | `AccountTab` (password change, sign out) | phone layout only |

Data, adapters, and security code are shared; phone components are
presentational wrappers so rules, rollback, validation, and routing stay in one
place.

### 2. Breakpoint mechanism

- One `useViewport()` hook based on `matchMedia("(max-width: 767px)")` decides
  the component tree where structure differs: shell, Overview, Session detail,
  Terminals, History, Recipes, More. CSS media queries at the same 767 px
  handle styling only. The breakpoint is one exported constant.
- The sidebar is not rendered below 768 px, so the icon rail never appears on
  phones. The existing 768–1,199 px collapse behaviour is unchanged.
- Rotation or resize across 768 px swaps trees. Live data stays in
  `LiveDataProvider`, which sits above the switch, so nothing refetches.
- A terminal reattaches. Attach returns the rendered screen snapshot, so the
  visible state is restored.
- Key-bar input is sent immediately, so there is no unsent buffer to lose.
- List scroll position is kept per route in memory, never in storage.

### 3. Navigation

| Tab | Route | Desktop equivalent |
|---|---|---|
| Overview ▦ | `/app/overview` | Overview |
| Terminals ❐ | `/app/split` | Split view |
| History ↺ | `/app/history` | History |
| Recipes ≡ | `/app/recipes` | Recipes |
| More ⋯ | `/app/more` | Settings (desktop `/app/more` redirects to `/app/settings`) |

Detail screens and their back targets:

- Session: `/app/sessions/:id` goes back to Overview.
- Terminal: `/app/sessions/:id/terminal` goes back to the Session. Desktop
  redirects this route to `/app/sessions/:id`.
- Project: `/app/projects/:id` goes back to Overview.
- Run Recipe: `/app/recipes/:id` goes back to Recipes.
- Settings screens: `/app/settings/:tab` goes back to More.

Deep links:
- A notification URL (`/app/sessions/<machine>:<id>`) is unchanged. The phone
  renders B4 if the Session is waiting with a prompt, B5 if it failed, and B3
  otherwise.
- The 4.C states for a missing Session and an offline or revoked Machine are
  kept, restyled for phones.
- Login return paths keep `safeReturnPath` unchanged.

Back behaviour:
- The back bar goes to the previous in-app entry when one exists. After a
  deep link (no in-app history), it goes to the parent route, so a tapped
  push never sends you out of the app.
- Browser back and swipe-back in Safari tabs follow history.
- Installed iOS PWAs have no system swipe-back, so the back bar is the
  guaranteed path.

### 4. Data gaps — no protocol or daemon change proposed

| Gap | Decision |
|---|---|
| C3: last two output lines | **Peek attach** for FULL Sessions whose card is on screen (IntersectionObserver), at most 4 at once. It uses the attach screen snapshot, reads the last two non-empty lines, and sends **no input and no resize**, so the real terminal size is never changed. It detaches when the card scrolls off. Sessions captured as COMMANDS, PROCESS or PROCESS_LOG show their current Command or last Event instead. |
| B3: "last output" age | Show **"LAST ACTIVITY"**, the newest of Session `updatedAt`, last Event time and last Command update. It's labelled honestly; it's not output age. A true last-output timestamp would be a later additive Session field and needs its own approval. |
| B5: "how it passed last time" | The existing `getHistory({ machine, query })` on the owning Machine, filtered in the client to the same Command text with exit 0. The link is hidden when there's no match. |
| E4: signed-in browsers | The design shows only "This phone · Active now · Sign out". That's the current browser session plus logout, which exist today. A password change already revokes every session (`auth/password.rs`). No list endpoint is needed. |

### 5. Sheets and confirmations

One `BottomSheet` component for C2 and D2:
- `role="dialog"`, `aria-modal`, labelled by its title;
- focus moves to Cancel and is trapped inside;
- Escape, a scrim tap, or a downward drag on the grabber closes it, all
  non-destructive;
- safe-area bottom padding;
- focus returns to the control that opened it.

Destructive actions (Send Ctrl-C, Kill, Revoke) happen only through an explicit
button inside the sheet. Ctrl-C asks for confirmation when the Session has run
longer than 60 seconds, and always for Kill. Revoke on phones uses the same
sheet.

### 6. Terminal on a phone

- `PhoneTerminal` fills the visual viewport: a 52 px header, xterm, then the
  key bar.
- A `--vvh` variable follows `window.visualViewport` (resize and scroll), so
  the key bar stays above the software keyboard and xterm refits when the
  keyboard opens or closes, and in landscape.
- Key bar: Ctrl-C (red, opens C2 per §5), Esc, Tab, ↑, ↓, y, n. Keys are
  44 px and the bar scrolls sideways. It reuses the `keybar.tsx` byte
  sequences.
- Read-only Sessions (finished, or reduced capture) show the retained or
  read-only view with no key bar.
- No terminal bytes or credentials are written to storage or URLs. The
  existing cookie-authenticated WebSocket is unchanged.
- Like the desktop terminal today, attaching from the phone resizes the shared
  PTY to the phone's columns, so a desktop view of the same Session reflows.
  Peek cards (§4) never resize.

### 7. Tokens and type

- Add `--destructive: #D12A20` in both themes.
- Add `--safe-top` and `--safe-bottom` from `env(safe-area-inset-*)`.
- Add the mobile type scale as classes used only by phone components.
- `web/index.html` currently has `width=device-width, initial-scale=1.0`
  without `viewport-fit=cover`, so `env(safe-area-inset-*)` reads 0 and the
  notch area is never used. Add `viewport-fit=cover`. The manifest is
  already `display: standalone` with no orientation lock, so landscape stays
  allowed.
- Desktop uses none of these, so its appearance is unchanged.

### 8. Test approach

- **Vitest** at a phone width (a `matchMedia` stub) for the components and
  rules listed in the prompt.
- **Routing tests** for every route and deep link, including same-numbered
  Sessions on two Machines.
- **Playwright** on the production build with two fake workers at 390×844,
  320×568, and 844×390, as a new `mobile` project:
  - the flows listed in the prompt;
  - an overflow check (`scrollWidth <= innerWidth` except chip rows and the
    key bar);
  - checks that browser storage and URLs hold no credentials;
  - one screenshot per screen saved as a reviewed artifact, not a pixel
    golden.
- The existing desktop Vitest and Playwright suites must pass unchanged.

## Questions for the owner

1. **Ctrl-C confirmation threshold:** confirm only after 60 s of running (the
   proposal), or always?
2. **C3 source:** peek attach for on-screen live cards (the proposal), or show
   only the current Command and last Event with no attach?
3. **"+" on Overview:** create a FULL Session on the control Machine and open
   C1, as desktop does today?
4. **B3 "LAST ACTIVITY":** accept the honest label for now, with a true
   last-output field as a later additive change?

## Implementation order (after approval)

1. Tokens, `useViewport`, `BottomSheet`, tab bar and back bar, phone shell.
2. Notification path: B2, the `PhoneSessionRow` overlap fix, B3, B4, B5, C1,
   C2, and deep-link rendering.
3. C3, D1, D2, D3, D4.
4. E1, E2, E3, E4.
5. A1, A2, B1.
6. Playwright mobile project, screenshots, `just check`.
7. Real-device acceptance.

## Proving tests (written first)

- [x] Tab bar, back bar, and `useViewport` switch (no icon rail below 768 px)
- [x] `PhoneSessionRow`: badge never on the title line; long Machine names truncate
- [x] Needs-you cards and waiting y/n honour capture capabilities
- [x] `BottomSheet`: focus trap, Escape, scrim, return focus, no single-tap destructive action
- [x] Ctrl-C threshold and Kill always confirm (phones expose no Kill action;
  Stop and the key bar's Ctrl-C share the 60 s rule; Revoke always uses a sheet)
- [x] Metric grid with and without training data; "LAST ACTIVITY" derivation
- [x] B3/B4/B5 selection by Session state; deep link for missing Session and offline or revoked Machine
- [x] Peek attach sends no input or resize, and stops when the card scrolls off
- [x] E3 chips: rules, Machine scoping, rollback on refusal
- [x] D4: offline Machines disabled; validation and port warnings kept
- [x] Routing for every desktop route and deep link; same-numbered Sessions never cross
- [x] Playwright `mobile` project at 390×844, 320×568, 844×390; screenshots; no overflow; no credential in storage or URLs
- [x] Desktop suites unchanged

## Automated evidence (2026-10-08)

- `pnpm exec vitest run`: 45 files, 196 tests passed. Phone tests are in
  `web/src/routes/app/phone/*.test.tsx`, `BottomSheet.test.tsx`,
  `peek.test.ts`, `peekHook.test.ts` (on-screen only, at most 4 peeks), and
  `PhoneLanding.test.tsx`.
- `pnpm exec playwright test` on the production build: 11/11 passed, including the
  `mobile` project (`web/e2e/mobile.spec.ts`) at 390×844, 320×568, and rotation
  to 844×390. That run checks the deterministic 60 s Ctrl-C rule (`page.clock`),
  that the phone Recipe run lands on `/app/sessions/<owner>:<n>`, that nothing
  overflows sideways, and that only `sentinal.theme` is in browser storage.
- Per-screen screenshots are written to `web/test-results/mobile-*/` (A1, B1, B2,
  B2 light, B2 at 320, B3, B3 at 320, B3 missing, C1, C1 landscape, C2, C3, D1,
  D2, D3, D4, D4 result, E1, E2, E3, E4). They were reviewed by eye. Fixes
  from that review: the "Remove device" button gained a style, and screenshots
  now wait for the screen to finish rendering and turn animations off.
- The enrollment spec now runs at 1280×900 because it proves the desktop
  layout; the phone Recipe run is covered by the mobile spec.
- `just check`: passed.
- Not covered by automation: A2 is part of A1 on phones (four stacked feature
  sections). B1 reuses the existing sign-in screen, whose own 640 px rules
  already fit a phone.

## Revision 2 (2026-10-12) — handoff v2 navigation and desktop Session page

Source: the owner's `sentinal-mobile-handoff 2/`. It was copied over the tracked
`sentinal-mobile-handoff/` (spec, design, new `reference/SentinalSession.dc.html`;
`support.js` unchanged).

**Phone navigation (MOBILE_SPEC.md → Navigation)**

| Before | After |
|---|---|
| Bottom tab bar (Overview, Terminals, History, Recipes, More) | No tab bar. `PhoneShell` shows a 48 px `TopBar` on top-level screens: logo, the needs-you count (→ `/app/overview?needs=1`), and ⌕ |
| Plain page titles | `PageTitle`: "Overview ▾" is a button that opens the Menu |
| — | **N1 `PhoneMenu`**: full-screen dialog; search focused; pages with meta and a 4 px accent current-page bar; Machine and project chips; ✕/Escape close; focus returns to the opener; focus is trapped |
| — | **N2** search (`menuSearch.ts`): Pages, Sessions, Machines, Recipes, and History (debounced `getHistory({ query })`), 5 per group, with an empty state |
| E1 More at `/app/more` | E1 Settings at `/app/settings` (`PhoneSettingsHome`): Machines with status, Projects, "Machines & tokens"/Notifications/Account, Theme, Sign out. `/app/more` redirects. Sub-screens back out with "‹ Settings" |
| Back bar only | `BackBar` also goes back on a left-edge swipe (start ≤ 24 px, ≥ 72 px right, < 48 px vertical drift) |

The needs-you filter shows only those Sessions, with a pressed "Needs you N"
chip; "All" clears it. Sticky action bars and pages now carry the bottom safe
area themselves, which the tab bar used to provide.

**Desktop Session page:**
- The page is a two-column grid. The terminal is the left half: sticky, full
  height, and `#0C0C0C` in both themes. The details scroll on the right.
- With several generations, earlier ones scroll in a strip above the newest,
  which fills the column and keeps the key bar.
- The xterm viewport no longer shows a black frame under its padding.

**Evidence:**
- Vitest: phone tests cover the top bar, Menu (focus, Escape, current page),
  grouped search, the needs filter, edge swipe, and the Settings redirect.
- `e2e/mobile.spec.ts`:
  - navigates only through the Menu;
  - checks the swipe back;
  - adds screenshots `n1-menu`, `n2-search`, `b2-needs-you` (when present),
    and `e1-settings`.
- `e2e/desktop-session.spec.ts` measures the halves at 1280, 1440, and 1920
  (sidebar open and collapsed) and at 768 and 1024. It also checks scroll
  stickiness, reflow, and the light theme.

## Real-device matrix (to fill during acceptance)

| Device | OS / browser | Install mode | Theme | Result |
|---|---|---|---|---|
| iPhone (owner) | to record | Home Screen PWA | light and dark | pending |
| Android phone | if available | Chrome install | — | unavailable unless supplied |
| Control | Mac mini, candidate to record | Tailnet HTTPS | — | pending |
| Workers | MacBook; pop-os (offline since 2026-10-07) | — | — | pending |
