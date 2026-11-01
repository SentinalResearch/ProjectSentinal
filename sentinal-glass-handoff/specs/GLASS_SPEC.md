# Sentinal — Glass shell spec (desktop ≥ 1024px)

Reference: `design/Sentinal Glass.dc.html` (01 Dashboard, 02 Prompt open, 03 Session). It replaces the sidebar. Earlier explorations are in `design/Sentinal Nav Options.dc.html` (chosen: 2a island + 2c prompt). Open both through a local server (`npx serve design`).

## 1. Tokens
```css
:root {
  /* surfaces */
  --bg-app:#09090B; --bg-deep:#060607; --bg-0:#0A0A0A; --term-bg:#0C0C0C;
  /* text */
  --text-1:#F2F2F0; --text-2:#A6A6A2; --text-3:#8A8A86; --term-fg:#D4D4D0;
  /* accent */
  --accent:#3D5AFE; --accent-fg:#8C9EFF;
  /* states (badge text is always #0A0A0A; always glyph + label) */
  --s-running:#29D35F; --s-waiting:#FFD000; --s-stalled:#FF8A00; --s-failed:#FF3B2F;
  --s-completed:#36C5E8; --s-idle:#9C9C98; --s-interrupted:#FF5CA8; --s-created:#B9A8FF; --s-disconnected:#8A8A86;
  --destructive-fg:#FF6B61;
  /* glass */
  --glass-bg:rgba(30,30,34,.55);
  --glass-bg-strong:rgba(30,30,34,.62);
  --glass-panel:rgba(28,28,32,.55);
  --glass-edge:rgba(255,255,255,.12);
  --glass-edge-soft:rgba(255,255,255,.08);
  --glass-blur:blur(24px) saturate(1.6);
  --glass-shadow:0 20px 60px -20px rgba(0,0,0,.9);
  --well:rgba(0,0,0,.35);       /* inset areas inside glass */
  --chip:rgba(255,255,255,.08); /* quiet buttons on glass */
}
```
Glyphs: RUNNING ● · WAITING ? · STALLED ‖ · FAILED ✕ · COMPLETED ✓ · IDLE – · INTERRUPTED ■ · CREATED + · DISCONNECTED ○ (outline).

## 2. Type
Archivo (UI), JetBrains Mono (commands, paths, ports, prompts, timestamps), `tabular-nums` everywhere.
Hero 72/64 800 −0.055em · session title 48/46 800 · big metric 104/88 800 · card title 22 800 · body 14–15 · overline 10–11 700 +0.08em uppercase.

## 3. Glass recipe
- Floating controls (island, prompt): `background: var(--glass-bg); backdrop-filter: var(--glass-blur); box-shadow: inset 0 0 0 1px var(--glass-edge), var(--glass-shadow); border-radius: 24px` (pills use height ÷ 2).
- Panels (machine lanes, feed, metric tiles): `--glass-panel`, blur 18px, edge `--glass-edge-soft`, radius 18–20px (tiles 14, inner cards 12, buttons 10).
- Attention cards: tinted, not blurred: `background: rgba(<state>, .07); box-shadow: inset 0 0 0 1px rgba(<state>, .35)`.
- Ambient light: at most two large, very soft radial glows per screen (blue ≤ .16 alpha, state colours ≤ .08). Never a hard gradient.
- Terminal surfaces are never glass: a solid `--term-bg`, so program colours stay true.
- Fallback when `backdrop-filter` is unsupported: raise the background alpha to .92.

## 4. Live island (always visible)
Fixed, top 16px, centred, 48px tall, radius 24.
Contents: logo 18 · current location (page name, or `machine › session` in mono + bold title) · divider · **live ticker** (up to 3 items: 8px state square + short text, by priority FAILED > WAITING > STALLED > RUNNING progress) · prompt button (`❯ ⌘K`).
- When space is tight, the ticker collapses to a yellow "N need you" chip.
- Clicking a ticker item opens that session. Clicking the location opens the prompt prefilled with `go `.
- The ticker updates live; a new FAILED or WAITING item enters with a 200ms fade. No other motion.

## 5. Prompt (on demand)
Open with ⌘K, `/`, or the island button. Bottom centre, 820px wide, 24–40px from the bottom. The page behind dims to 28 % and blurs 2px.
- Input: 60px pill, edge `rgba(140,158,255,.6)` 1.5px, mono 17px, `❯` in accent, block cursor.
- Suggestions panel above it, grouped: ANSWER · GO · RUN · FIND. The selected row gets an accent tint and a 3px left bar.
- Footer hints: `↵ run · tab complete · ⌘↵ open in split · esc close`.
- Grammar (no AI; deterministic matching):
  - `go <machine>[/<session>]`: navigate
  - `y|n <session>`: answer a WAITING prompt (sends key + Enter)
  - `run "<recipe>" [on <machine>]`: opens the run form prefilled; never runs without confirmation
  - `find <text>`: history search
  - `split <a> <b> …`: open split view
  - `kill <session>`: always opens the confirm dialog
- Bare text runs a fuzzy search across sessions, machines, recipes and history.

## 6. Sheets (back history)
Leaving a page pushes it as a sheet on the right edge: a 72–120px sliver, radius 18 on the left, `rgba(26,26,30,.8)`, vertical label. Show at most 3, offset 40px each, opacity .85 / .55 / .3.
Click a sheet, or press ⌘[, to bring it forward. The current page sits above with `box-shadow: 20px 0 60px -10px rgba(0,0,0,.9)` and a right radius of 18.

## 7. Dashboard (01)
Grid `minmax(0,1fr) 360px`, padding `92px 40px 32px` (clears the island).
1. Header: date · session count; the hero shows "N need you." (or "All quiet." when there are none). Hint: "press / to do anything".
2. **Needs-you row:** 3 columns of attention cards with inline actions. WAITING: prompt well + y / n. FAILED: summary + Open / Last passing run. STALLED: evidence + Open terminal / Kill… (confirm).
3. **Machine lanes:** a glass panel per machine, grid `200px 1fr`. Left: name, hardware (mono), two mini resource bars (56px). Right: session cards (min 150, max 280) with badge, duration, title and a 20-bar sparkline (accent for the focused or training session, state tint otherwise). Offline machines at 50 % opacity with a one-line note.
4. **While you were away** (right column): time · state square · headline · mono detail.

## 8. Session (03)
Page grid `1fr 1fr`. Left: terminal at full height (76px top spacer under the island), with a key bar of 10px-radius keys (Ctrl‑C tinted red and confirmed). Right: breadcrumb + command, title, badges, 104px progress, a 3-tile metric row, events panel.

## 9. Rules kept from the base system
States never colour-only · numbers in tabular figures with units · commands monospace · nothing destructive in one click · Ctrl‑C / Kill always confirm · motion only to show liveness · respect `prefers-reduced-motion` (no fades; the prompt opens instantly).

## 10. Mobile
Phones keep `MOBILE_SPEC.md` (top bar + menu). The island maps to the mobile top bar; the prompt maps to the mobile menu/search.
