# Sentinal — mobile design spec (for coding agents)

Reference design: `Sentinal Mobile.dc.html` (open via a local server, e.g. `npx serve .`). It contains 17 phone screens at 390 × 844. Rebuild them in the real app (React + xterm.js PWA) as responsive layouts. Use the rules here; the HTML file is the visual reference.

## Breakpoint
- `< 768px` → mobile layout below. `≥ 768px` → the existing desktop layout (sidebar).
- Never shrink the desktop sidebar into an icon rail on phones. Replace it with the bottom tab bar.

## Tokens (CSS variables)
| token | light | dark |
|---|---|---|
| --bg-0 | #F7F6F2 | #0A0A0A |
| --bg-1 | #EFEEE9 | #111111 |
| --bg-2 | #E4E3DD | #1C1C1C |
| --text-1 | #0A0A0A | #F2F2F0 |
| --text-2 | #4A4A46 | #A6A6A2 |
| --text-3 | #5E5E5A | #8A8A86 |
| --line | #D9D8D2 | #262626 |
| --line-strong | #8A8984 | #6B6B67 |
| --accent | #2A44E0 | #3D5AFE |
| --accent-fg | #2A44E0 | #8C9EFF |
| --destructive | #D12A20 | #D12A20 |

State colors (both themes; badge text is always #0A0A0A): RUNNING #29D35F ●, WAITING #FFD000 ?, STALLED #FF8A00 ‖, FAILED #FF3B2F ✕, COMPLETED #36C5E8 ✓, IDLE #9C9C98 –, INTERRUPTED #FF5CA8 ■, CREATED #B9A8FF +, DISCONNECTED transparent with a 1.5px #8A8A86 inset outline ○. Always glyph + label, never color alone.

Terminal surface is always #0C0C0C with text #D4D4D0, in both themes.

## Type
- UI: Archivo 400–800. Mono: JetBrains Mono (commands, paths, ports, prompts, durations in lists).
- `font-variant-numeric: tabular-nums` everywhere.
- Mobile scale: page title 34/36 800 (-0.04em) · detail title 30/32 800 · section heading 17 800 · row title 16–17 700 · body 15/21 · meta 12–13 · overline 11 700 uppercase letter-spacing 0.08em (--text-3) · big metric 64/60 800 (-0.05em).

## Layout rules
- Screen padding 20px horizontal. Status-bar safe area on top; bottom bars add 34px safe-area padding (`env(safe-area-inset-bottom)`).
- No border-radius on UI (square, Swiss). Section headers use a 2px --text-1 bottom rule; rows use a 1px --line bottom rule.
- Touch targets ≥ 44 × 44. Primary buttons 52px tall, full width.
- **Session row (mobile):** column, gap 5px.
  1. Line 1: state badge (left, 22px tall) + mono duration (right).
  2. Line 2: title 16/700.
  3. Line 3: mono command 12px --text-3, single line, ellipsis.
  The badge never shares a line with the title. This fixes the overlap bug.
- Long machine names (e.g. `sohiths-MacBook-Pro.local`): single line + ellipsis; never wrap mid-word.
- Horizontal chip rows (machines, history filters) scroll sideways; they don't wrap.

## Navigation
Bottom tab bar, 83px (49 + safe area), top 1px --line border, 5 equal columns: Overview ▦ · Terminals ❐ · History ↺ · Recipes ≡ · More ⋯. Active tab: --accent-fg, weight 800. Others --text-3.
Detail screens get a 44px nav bar with a "‹ Parent" back link in --accent-fg and an optional ⋯ menu.

## Screens
- **A1 Landing (public):** 56px header (logo, theme icon, "Sign in" text button); left-aligned hero 46/44 headline; buttons stacked full width (Sign in = accent, Set up this server = outline); small native "needs you" preview list, not a scaled desktop mock.
- **A2 Features:** stacked sections "01 / 04 …" with native mobile rows and the 68 % block. No 3D-tilted desktop screenshots on phones.
- **B1 Sign in:** logo 64, "Welcome back.", password field 56px with Show toggle, Sign in button.
- **B2 Overview:** title + "+" (new session); machine chips row; "Needs you" heading with a yellow count; needs-you cards (bg-1, 3px top inset in the state color). The WAITING card shows the prompt in mono with inline y / n buttons (52 × 48). Then machine groups with session rows.
- **B3 Session detail:** breadcrumb, title, badges (state, health outline, duration), command; 68 % + progress bar; 2×2 metric grid (LOSS, GPU, VRAM, LAST OUTPUT); health evidence box; events list. Sticky bottom: Open terminal (accent, flex) + Stop (destructive outline, 96px).
- **B4 Waiting:** big mono prompt on a terminal-dark block; y / n 80px-tall buttons; Open terminal outline.
- **B5 From a notification (failed):** red header band with "✕ FAILED · RTX-PC" and the failure summary; 3-cell stat grid; last output; Open terminal + "See how it passed last time".
- **C1 Terminal:** dark full screen; 52px header (‹ Detach, title + machine/path, badge); xterm fills the space; key bar above the keyboard: Ctrl‑C (red) Esc Tab ↑ ↓ y n, 44px keys, scrolls sideways.
- **C2 Ctrl‑C confirm:** bottom sheet (grabber, title, explanation with command + runtime, destructive "Send Ctrl‑C", outline Cancel). Required for Ctrl‑C on long jobs and for Kill.
- **C3 Terminals (split view on phone):** stack of live terminal cards (slim state header + last 2 lines); tap to open full screen. The focused card gets an accent outline.
- **D1 History:** search field; filter chips row; date group heading; rows = 28px state square + mono command (ellipsis) + meta (machine · project · duration) + ⋯ menu (Save as recipe, Open, Copy).
- **D2 Filters sheet:** Machine / Project / Result / Date chip groups that wrap; Reset; "Show N results".
- **D3 Recipes:** grouped by category; rows = name, mono template (ellipsis), machine · parameter count, chevron.
- **D4 Run recipe:** parameter fields (mono, "default" hint), 3-way target machine segmented control (offline machines disabled), command preview, sticky "Run on <machine>".
- **E1 More:** Machines (status), Projects, Settings rows, Theme segmented (Light / Dark / Auto), user + Sign out (destructive text).
- **E2 Machines & tokens:** Pair a machine button; machine cards with name, role tag (CONTROL / WORKER), hardware, status, "Revoke token" (destructive, confirm).
- **E3 Notifications:** device push toggle card; per-session rows with wrap-around toggle chips (Finished, Error, Stalled, Checkpoint, Progress). Replaces the desktop checkbox table on phones.
- **E4 Account:** change password, signed-in browsers with Sign out.

## Behavior
- States: default, pressed (bg-2 / 0.96 scale on buttons), disabled (--text-3, no pointer), loading (button label → "…" and disabled).
- Motion only to show liveness (subtle pulse on RUNNING). Respect `prefers-reduced-motion`.
- Nothing destructive happens on a single tap.
