# Master prompt: paste into your coding agent

You are updating **Sentinal** (React + xterm.js PWA) to the glass design in this folder.

Read first: `specs/GLASS_SPEC.md`, then `specs/MOBILE_GLASS_SPEC.md`. Visual references are in `design/` (run `npx serve design`, then open `Sentinal Glass.dc.html` and `Sentinal Glass Mobile.dc.html`). The specs win over the pictures if they disagree.

Work in two tracks, each as small PRs:
1. **Desktop (≥ 1024px):** follow `prompts/1-desktop.md`.
2. **Mobile (< 768px):** follow `prompts/2-mobile.md`. Reuse the desktop glass primitives and tokens; don't duplicate them.

Between 768 and 1023px, use the desktop shell with the dashboard's right column stacked under the main content.

Ground rules (all tracks):
- CSS variables from GLASS_SPEC §1 only; no new UI library.
- States are never colour-only (glyph + label). Commands, paths, ports and prompts in monospace. Tabular numerals.
- Nothing destructive in one action: ^C on long jobs, Kill and Revoke always confirm.
- The terminal surface is solid `#0C0C0C`, never glass.
- Respect `prefers-reduced-motion`; motion only shows liveness.
- Keep existing data and APIs; this is a UI change.

When the specs change later, read `CHANGELOG.md` and apply only the new entries.

Report back after each PR: what changed, screenshots at 390 × 844 and 1440 × 900, and anything in the spec you couldn't follow.
