# Questions

Ambiguities hit while building `design_handoff_sentinal` against this codebase.
Marked `// QUESTION:` at the point in the code where a judgment call was made.

## Open

1. **Assumptions in `fixtures.json → _assumptions`** were invented by the designer
   and need product confirmation (kept in `src/data/config.ts`, not hard-coded):
   pair command `sentinal-agent pair <TOKEN>`, token mask `sa_••••xxxx`, pairing
   TTL 10 min, setup password ≥ 12 chars,
   `cargo build` on MacBook, relative history dates (Today / Yesterday / 3 days ago).
2. **Session-expired trigger — now wired to real authentication.**
   `SentinalSettings.dc.html`'s own Account tab has the answer: a
   "PROTOTYPE: Simulate session expiry" dev button (`expire` handler), not a
   real timer or daemon signal. `AccountTab.tsx` ports this literally — the
   button calls `AuthContext.markExpired()`, which flips `status` away from
   `signedIn` and lets `RequireAuth` redirect to `/login` where the `expired`
   banner renders. In API mode, the daemon now issues 24-hour browser sessions,
   rejects expired HTTP access, and revalidates active WebSockets; a failed
   session check returns the UI to this expired state. The prototype button is
   retained only as a quick visual test.
3. **Ctrl-C vs. Kill.** The design only shows a Ctrl-C confirm dialog
   (`sendSignal(id, "SIGINT")`). `sendSignal` also accepts `"KILL"` for the
   destructive-entry-point pattern described in Settings/Session actions, but no
   screen in the handoff shows a distinct "kill" UI — currently mocked as
   behaviorally identical to SIGINT (state → `INTERRUPTED`). Confirm whether a
   harder kill path is needed.
4. **New machine's identity after pairing.** The pairing flow's "waiting → connected"
   step is mocked as: after ~5s, a machine is added with `os: "Linux"` and
   `hardware: "Paired machine"` (the design doesn't specify what a freshly paired
   machine reports for OS/hardware before its first real check-in).
5. **Maskable PWA icon.** No maskable-safe variant of the mark exists in
   `assets/` (the artwork fills the viewBox edge-to-edge). `manifest.webmanifest`
   reuses the standard icon for `purpose: "maskable"` since it already has a solid
   background fill — a real safe-zone variant should replace it if the design
   system adds one.
6. **Success-animation vs. redirect timing (auth).** The prototype's login/setup
   flow shows the mark's success (grow) animation for ~520ms *before* firing
   `onLogin()`/navigating. `AuthContext.login`/`.setup` (`src/context/AuthContext.tsx`)
   now resolve immediately on success (so `LoginPage`/`SetupPage` can start that
   animation) but delay committing `signedIn` into shared state — which is what
   `PublicAuthGuard` reacts to — by 550ms, so the redirect doesn't cut the
   animation off. This is a deliberate behavior change from the original
   (synchronous) context shape; flag if a different mechanism (e.g. the guard
   itself deferring) is preferred.
7. **`expired` auth mode's relationship to `login`/`wrong`.** The prototype only
   ever reaches `expired` via a manual state switcher (no real trigger existed
   even in the prototype). It's rendered here as a banner on the normal login
   form (`status === "expired"`) rather than a separate auto-dismissing screen —
   matches the prototype's actual behavior (no auto-timeout there either), but
   worth confirming nothing more elaborate was intended.
8. **⌘K / Ctrl-K behavior.** README says "Search history ⌘K (→ History)"; the
   prototype's shortcut handler (`SentinalApp.dc.html`) just navigates to
   `app/history` on keydown, with no palette/overlay markup anywhere in the
   handoff. Implemented literally as a navigate-only shortcut
   (`AppShell.tsx`) — if an actual command-palette overlay (search-as-you-type
   over history) was intended, it isn't specified anywhere in this handoff.
9. **Project id derivation.** Sessions/history only carry a project *name*
   string (`"TIM"`, `"MentalAnimAItion"`), never an id — but the prototype's
   sidebar links to `app/project` with hardcoded ids `tim`/`maa` that don't
   obviously derive from the names (`maa` isn't a slug of
   `MentalAnimAItion`). `slugifyProject()` in `src/data/format.ts` lowercases
   and hyphenates the name instead (`mentalanimaition` for that one) — now used
   consistently by the sidebar's project links, the Project page
   (`/app/projects/:id`), and History's `?project=` deep link, so the ids match
   everywhere even though they read differently from the prototype's own
   abbreviations.
10. **Health card values aren't a fixed enum.** `SessionHealth`'s
    `activity`/`resources`/`stability`/`errors` fields were typed as a
    `HealthLevel` union (`'GOOD'|'WATCH'|'BAD'|'NONE'`) that doesn't match the
    design — SentinalSession.dc.html's own demo data uses free-form strings
    colored by a lookup with a text-1 fallback (`'NONE 14m'`, `'CPU 0 %'`, even
    a literal failure count `'3'`). Retyped to `string` in `src/data/types.ts`;
    `evidence` was also required but the STALLED fixture (`compression`) has no
    `evidence` array at all — made optional.
11. **"Run again" has no adapter method.** The FAILED panel's "Run again"
    button (Session Detail) has nothing to call — `DataAdapter` has no
    retry/rerun-command method, and `sendInput` only makes sense for a WAITING
    prompt. Left as a no-op (`SessionDetailPage.tsx`'s `rerun()`) rather than
    inventing a new adapter method or fake behavior.
12. **Terminal architecture: real xterm, simulated stream in mock mode.** There's
    no real PTY behind `VITE_DATA=mock`, so `TerminalPane` (`src/terminal/`)
    still mounts a real `@xterm/xterm` instance (per the brief's rule 2) but
    replays the session's `events` as scrollback and turns typed lines into
    `dataAdapter.sendInput()` calls, instead of a real byte stream. The real
    (`VITE_DATA=api`) path uses `useRealTerminalSession`, extracted from the old
    `src/App.tsx` (Milestone 1), and is exercised by the fake-daemon Playwright
    smoke test. Two things worth confirming: (a) real daemon
    session ids are numeric (`protocol/SessionId.ts`) but the mock's `Session.id`
    is a string slug (`"qwen"`) — `TerminalPane` always takes a string and
    `Number()`s it for the real path; (b) each `TerminalPane` opens its own
    WebSocket, so Split View would open N connections to the same daemon
    instead of sharing one (see `FUTURE.md`).
13. **Raw Ctrl-C keystroke vs. the Ctrl-C button.** Rule 5 ("destructive actions
    always go through a confirm step") is implemented as gating the app's
    explicit Ctrl-C affordances (key-bar button, header "Ctrl‑C…") behind
    `ConfirmDialog` — a raw Ctrl-C keystroke typed directly into the attached
    terminal passes straight through as normal terminal input, same as any
    terminal emulator, and isn't intercepted. Confirm this reading is correct;
    the alternative (intercepting every Ctrl-C keypress in the terminal itself)
    would break normal interactive shell use.
14. **jsdom needs `matchMedia`/`ResizeObserver` polyfills for `@xterm/xterm`.**
    Neither exists in jsdom; without them `TerminalPane` crashed every test that
    rendered it. Added minimal no-op polyfills to `src/test-setup.ts` — real
    browsers already have both, so this only affects the test environment.
15. **History's original mock `store.history` had no machine/project/id.** `fixtures.json`'s
    `history` array is just `{ok, command, duration}` — nothing for the
    Machine/Project filter chips to key off. `SentinalHistory.dc.html`'s own
    working prototype (not just the static fixture) has a richer 6-row dataset
    that enriches exactly these commands with machine/project/session id, plus
    two rows for the live `vllm`/`npm` sessions not present in the fixture at
    all — mirrored in `src/data/mock/store.ts`'s `buildHistory()` rather than
    inventing new values. These synthetic running/waiting rows aren't kept in
    sync if a session's state changes later (e.g. `qwen` finishing wouldn't
    update its history row) — acceptable for a mock, but worth knowing. API
    mode now maps real protocol `Command` records, including Machine, Session,
    cwd, timestamps, and outcome, and delegates text queries to SQLite FTS5.
16. **History's date filter uses day-count buckets, not the relative-date
    labels.** `config.historyRelativeDates` (Today/Yesterday/3 days ago) is a
    flagged *display* assumption (#1). The **filter itself** mirrors
    `SentinalHistory.dc.html`'s actual working `dateMax` map — Today / 7 days /
    30 days / All time, a cumulative day-count threshold — which is a
    confirmed behavior from the live prototype, not a guess, so it's
    implemented as-is in `HistoryPage.tsx` rather than reusing the relative
    labels for both purposes.
17. **Settings → Account: no invented multi-browser rows.** AGENT_PROMPT.md's
    "Mock now, build for real later" list says multi-browser sessions should
    get placeholder UI behind a flag — but `SentinalSettings.dc.html`'s Account
    tab, the actual design, renders exactly one row ("This browser"), nothing
    more. `AccountTab.tsx` follows the design literally rather than inventing
    extra `MOCK`-badged rows the design doesn't show, per rule 9 ("don't invent
    product features beyond the designs"). `flags.multiBrowserSessions` exists
    but is unused by this tab for that reason — worth confirming this reading
    is correct before a real multi-device session list is designed.
18. **Revoke's confirm pattern, resolved.** `SentinalSettings.dc.html`'s own
    prototype shows Revoke as an *inline row-swap* confirm (`Rotate`/`Revoke…`
    replaced in place by `Revoke?` + `Revoke`/`Keep`), not the modal
    `ConfirmDialog` Session Detail's Ctrl-C uses. `MachinesTab.tsx` implements
    the inline pattern; `ConfirmDialog` is intentionally not reused here — two
    different confirm affordances by design, not an inconsistency.
19. **Pairing step 3 detection replaces the prototype's manual button.**
    `SentinalSettings.dc.html` has a "Prototype: simulate agent connected"
    button because its own mock had no real timer. This build's mock adapter's
    `pairMachine` really does add the machine and emit `MACHINE_UPDATED` after
    ~5s, so `PairMachineDialog` watches `useLiveData()` for a machine matching
    the just-paired name and advances to step 3 automatically instead — a
    strictly better mechanism than the prototype's own placeholder, not an
    ambiguity, but noted since it's a visible behavior difference from the
    design file.
20. **Landing page reduced-motion resting state for the showcase section.**
    `SentinalLanding.dc.html`'s own `prefers-reduced-motion` handling freezes
    its scroll progress at a hardcoded `p: 0.5`, which lands mid-way through
    the "How is it going?" step with the rest of the mock dimmed to 0.18 —
    arguably not a clean "final state" (rule 7 wants everything shown, not one
    section highlighted and the rest dimmed forever). `LandingShowcase.tsx`
    instead fixes `p` at `0.25` under reduced motion: past the 0.22 intro
    threshold (mock fully flat, full opacity) but before the first step
    threshold at 0.28 (`step = -1`, so nothing is dimmed and the first
    question is shown statically, un-cycling). A deliberate deviation from the
    prototype's own literal reduced-motion value, made in service of rule 7,
    not an unresolved ambiguity — flagging in case a different fixed resting
    state was actually intended.
21. **Recipes-section typing omits the prototype's mid-line pauses.**
    `SentinalLanding.dc.html`'s single shared typed-character counter has
    built-in pauses (~6 and ~14 "silent" ticks) between the comment line, the
    rendered command, and the `Serving on :8000` line. `LandingRecipes.tsx`
    reuses the existing `useSequentialTyping` hook (built for the auth
    screens, explicitly meant to be shared) instead of reimplementing the
    gapped-counter logic, which types each line back-to-back with no pause.
    A deliberate simplification for reuse, not a design ambiguity — the visual
    result (three lines typed in sequence at 28ms/char) is effectively the
    same, just without the brief pauses between lines.
22. **Split view has no phone treatment in the design.** 2-4 side-by-side xterm
    panes is inherently a wide-viewport idea, and nothing in the handoff
    specifies a phone behavior for it. `SplitViewPage.module.css` always stacks
    every open pane into a single column at a fixed `70vh` height on phone
    (scrollable page) rather than trying to preserve the 2/3/4-column grid at
    ~170px-wide columns — the layout picker still controls how many panes are
    open, it just no longer controls their arrangement below 640px. Reasonable
    default, not a confirmed design intent — a single-pane-with-switcher
    pattern might be preferred if this screen gets a real phone design later.
