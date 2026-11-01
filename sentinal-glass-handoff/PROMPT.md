# Prompt for your coding agent

Paste the block below. Attach this whole folder.

---

Replace Sentinal's desktop sidebar navigation with the **glass shell** described in `GLASS_SPEC.md`. The visual reference is `design/Sentinal Glass.dc.html`; serve it locally with `npx serve design`.

Do it in this order, one PR each:

1. **Tokens + glass primitives.** Add the CSS variables from spec §1. Create `<GlassPill>`, `<GlassPanel>` and `<AttentionCard state>` following §3, including the no-`backdrop-filter` fallback.
2. **Shell.** Remove the sidebar at ≥ 1024px. Add `<LiveIsland>` (§4), fed by the existing session store; the ticker priority is FAILED > WAITING > STALLED > RUNNING. Pad page content 92px at the top.
3. **Prompt.** `<PromptBar>` (§5): ⌘K, `/` and the island button open it. Implement the deterministic grammar (`go`, `y|n`, `run`, `find`, `split`, `kill`) with fuzzy fallback. `run` opens the prefilled recipe form; `kill` opens the existing confirm dialog. Nothing executes without the same confirmation it has today.
4. **Sheets.** A back-history stack (§6): max 3, ⌘[ to pop, click to restore. Keep the page state (scroll, terminal attachment) when restoring.
5. **Dashboard.** Rebuild the Overview as §7: needs-you cards with inline y / n, machine lanes with sparklines (last 20 samples of output rate or the progress metric), and the "While you were away" feed from the events journal.
6. **Session page.** §8: full-height terminal on the left, details on the right; xterm fit addon + ResizeObserver.

Constraints: plain CSS variables (no new UI library); keyboard reachable (focus rings on glass use `--accent-fg`); `prefers-reduced-motion` respected; mobile (< 768px) unchanged and still follows `MOBILE_SPEC.md`.

Done when: no sidebar on desktop; every page is reachable from the prompt; answering `y` to a WAITING session works from any page in 3 keystrokes (`/`, `y`, `↵`); Lighthouse accessibility ≥ 95.
