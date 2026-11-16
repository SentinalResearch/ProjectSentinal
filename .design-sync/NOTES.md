# design-sync notes (Sentinal web UI)

Run every command from the repo root. Config home is the repo root; the package being synced is `web/design-system/`.

## How this repo is synced

- The web app (`web/`) is an app, not a library: it has no library `dist/` and no Storybook. `web/design-system/` is a tiny entry package (`sentinal-ui`) that re-exports `web/src/components/*` and imports `web/src/styles/global.css`. The converter bundles straight from that TypeScript source (`cfg.entry`).
- Before the converter, run `cfg.buildCmd` (`web/node_modules/.bin/tsc -p web/design-system/tsconfig.json`). It emits `.d.ts` into `web/design-system/types/` (gitignored), which is where the prop contracts come from.
- Converter: `node .ds-sync/package-build.mjs --config .design-sync/config.json --node-modules web/node_modules --out ./ds-bundle` (same `--node-modules` for `resync.mjs`).
- A new component in `web/src/components` needs a line in `web/design-system/index.ts`, or it is not synced.
- Playwright for the render check: `npm i playwright@1.63.0` inside `.ds-sync/` matches the cached chromium-1243 in `~/Library/Caches/ms-playwright` (same version the web app pins).
- Tokens and fonts are not separate files: they are inside `_ds_bundle.css` (fonts as data URIs from @fontsource), so `tokens/` and `fonts/` are empty. That is expected.

## Decisions

- `MockBadge` and `PwaUpdatePrompt` are excluded (`componentSrcMap: null`). They render nothing outside a mock dev build / a waiting service worker, so they cannot be previewed or used in a design. Still exported from the bundle.
- `ThemeProvider` is exported but not a component card.
- Every preview starts with `document.body.style.background = "var(--bg-0)"`. The preview card hard-codes a white page, and Sentinal is dark by default with near-white text; without the line all text is invisible. Keep it in new previews.
- `ConfirmDialog` and `BottomSheet` are fixed overlays: `cardMode: "single"` with a viewport, and the ConfirmDialog preview wraps in a `minHeight` box so the centred panel is not cropped.
- Not previewable statically: HoldButton's holding fill, CopyButton's copied/failed states, Button's spinner animation, OutputSearchToggle's error line.

## Known render warns

- None. (`[GRID_OVERFLOW]` on PromptChoices, AttentionCard, GlassPanel, GlassPill was resolved with `cardMode: "column"`.)

## Re-sync risks

- `dtsPropsFor` is hand-written for Button, Input, AgentBadge, PromptChoices, OutputSearchToggle, GlassPill, GlassPanel, AttentionCard (the extractor drops native attributes, generics, and the large `Session` type). If those components' props change, update the entries by hand; nothing detects the drift.
- `.design-sync/conventions.md` lists token names; re-validate against `ds-bundle/_ds_bundle.css` after token changes.
- PromptChoices and OutputSearchToggle bundle the app's API adapter; clicking them in a design makes a request that fails. If `web/src/data` gains import-time side effects, previews may start throwing.
- The build assumes Node 24, pnpm 10.17.1 and an installed `web/node_modules` (`pnpm i --frozen-lockfile` in `web/`).
