# Building with Sentinal UI

Sentinal is a terminal-session monitor. Its look: dark by default, square corners, heavy Archivo type, mono for anything a terminal printed, and saturated state colours. Glass (blurred, rounded) is only for floating chrome and panels.

## Setup

- Link `styles.css` once. It sets the page itself: `body` gets `--bg-0`, `--text-1`, `--font-ui` at 14px/20px, so components need no wrapper to be styled. Do not put components on a white or light custom background: text is near-white (`--text-1`) and disappears. Keep the page on `--bg-0` (or `--bg-app` behind glass).
- Dark is the default. For light, set `data-theme="light"` on `<html>`; every token re-resolves. `ThemeProvider` (with `useTheme()`) from `window.SentinalUI` does this for you and is optional.
- Fonts ship inside the stylesheet: Archivo (400 500 600 700 800) and JetBrains Mono (400 500 700). Do not load other fonts.
- `ConfirmDialog` and `BottomSheet` are fixed, full-viewport overlays shown only while `open` is true. `PromptChoices` and `OutputSearchToggle` call the Sentinal API when clicked; in a mock-up they are for display.

## Styling idiom: CSS custom properties

Components carry their own styles. There are no utility classes; for your own layout use inline styles or your own CSS with these tokens:

| Family | Tokens |
|---|---|
| Surfaces | `--bg-0` (page) `--bg-1` `--bg-2` `--bg-app` `--bg-deep` `--well` `--chip` `--chip-hover` `--scrim` |
| Text | `--text-1` (primary) `--text-2` (secondary) `--text-3` (muted) `--ink` (text on a state fill) `--on-accent` |
| Lines | `--line` `--line-strong`; widths `--rule` (1px) `--rule-heavy` (2px) |
| Accent | `--accent` (fills) `--accent-fg` (text, focus rings) `--destructive` `--destructive-fg` |
| State fill / text | `--s-running` / `--s-running-fg`, and the same pair for `created` `idle` `waiting` `stalled` `completed` `failed` `interrupted`; `--s-disconnected-fg` |
| Severity | `--sev-info` `--sev-warning` `--sev-error` |
| Space | `--space-1` 4px, `-2` 8, `-3` 12, `-4` 16, `-5` 20, `-6` 24, `-8` 32, `-10` 40, `-12` 48 |
| Radius | `--radius` (0, the default for everything) `--radius-glass` 24px `--radius-panel` 18px `--radius-tile` 14px `--radius-card` 12px `--radius-key` 10px |
| Glass | `--glass-bg` `--glass-bg-strong` `--glass-panel` `--glass-edge` `--glass-edge-soft` `--glass-shadow` `--glass-blur` `--glass-blur-panel` `--glow-accent` |
| Type | `--font-ui` `--font-mono` |
| Terminal | `--term-bg` `--term-fg` (always solid, never glass) |

Global helper classes: `mono` (mono font), `rule` and `rule-heavy` (on `<hr>`), `visually-hidden`.

Rules of the system: state is never colour alone, so show a `StateTag` (fill + glyph + label). One `primary` Button per view. `destructive` Buttons live only inside a `ConfirmDialog`; on phones use `BottomSheet` with a `HoldButton`. Commands, paths and program output are mono. Use the product's words: Machine, Project, Session, Command, Event.

## Where the truth lives

Read `styles.css` and the `_ds_bundle.css` it imports for every token value (dark and light). Each component has `components/<group>/<Name>/<Name>.prompt.md` and `<Name>.d.ts`; groups are `general` and `glass`.

## Example

```jsx
const { AttentionCard, StateTag, AgentBadge, Button } = window.SentinalUI;

<AttentionCard state="WAITING" style={{ width: 380, padding: "var(--space-4)", display: "grid", gap: "var(--space-3)" }}>
  <div style={{ display: "flex", alignItems: "center", gap: "var(--space-2)" }}>
    <StateTag state="WAITING" size="sm" />
    <AgentBadge agent={{ agent: "CLAUDE", working: false, toolUses: 14 }} />
    <span style={{ marginLeft: "auto", color: "var(--text-3)", fontSize: 12 }}>studio · 4m</span>
  </div>
  <h3 style={{ fontSize: 16, fontWeight: 700 }}>Claude Code · sentinal</h3>
  <span className="mono" style={{ padding: "var(--space-2) var(--space-3)", background: "var(--well)", borderRadius: "var(--radius-key)", fontSize: 13, color: "var(--text-2)" }}>
    Do you want to run cargo test --workspace?
  </span>
  <Button variant="primary">Open Session</Button>
</AttentionCard>
```
