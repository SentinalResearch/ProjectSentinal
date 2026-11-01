# Prompt for your coding agent (mobile)

Paste the block below and attach this folder.

---

Update Sentinal's phone layout (< 768px) to the **glass mobile shell** and add a **built-in terminal keyboard**. Follow `MOBILE_GLASS_SPEC.md`. Visual reference: `design/Sentinal Glass Mobile.dc.html` (serve with `npx serve design`). Shared tokens and glass primitives are in `GLASS_SPEC.md` §1–3; reuse the desktop components if they already exist.

One PR each, in this order:

1. **Shell:** `<MobileIsland>` (top) and `<PromptPill>` (bottom) per spec §1. Remove the mobile top bar and menu. Content padding 112 / 110px + safe areas.
2. **Dashboard:** spec §2, using the same data as the desktop dashboard.
3. **Keyboard component:** `<TermKeyboard layer onBytes>` per §4: chip strip, accessory row, letters / symbols / ctrl layers, bottom row, latch/lock modifiers, key repeat, space-swipe cursor, haptics. Pure UI; it emits byte strings.
4. **Terminal integration:** set `inputmode="none"` on xterm's helper textarea; pipe `onBytes` to the PTY websocket; the `⌨︎` toggle; auto-hide when a hardware keyboard is detected (first physical `keydown`). Refit xterm when the panel shows or hides.
5. **Prompt sheet:** §3, using `<TermKeyboard>` without the accessory row; the same grammar as desktop.
6. **^C confirm:** §5, hold-to-send on jobs ≥ 30s.

Constraints: no new UI library; the keyboard must not trigger the OS keyboard on iOS Safari or Android Chrome (test both); 60 fps typing (no React re-render per key on the terminal path); `prefers-reduced-motion` respected.

Done when: typing `tail -f train.log 2>&1 | grep loss⏎` takes fewer taps than with the system keyboard; ^C, ^D, ^L, ^R and arrows work in bash, vim and python REPLs; the OS keyboard never appears in the terminal; desktop is unchanged.
