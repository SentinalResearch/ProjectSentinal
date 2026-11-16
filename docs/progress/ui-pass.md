# UI pass — numbered problem list from real use (task A3)

Screenshots arrive from the owner (`screenshot_issues/` is never committed);
each problem gets a number, a fix in the owner's priority order, and a test
where one is possible.

## Fixed

1. **Health charts render as empty black boxes for machines with one sample.**
   `HealthChart`/`MetricsChart` drew only a `polyline`, and a one-point
   polyline paints nothing, so e.g. the control's typing-delay chart was a
   black box with a legend. Fix: a marker circle per sample and single-sample
   series are centered (`web/src/routes/app/HealthChart.tsx`,
   `MetricsChart.tsx`; tests in `HealthChart.test.tsx`, `MetricsChart.test.tsx`).
2. **"Save as recipe" Name/ID fields rendered as browser-default white blocks**
   on the dark theme — the inputs had no class. Fix: `.saveInput` dark field
   style (`RecipesPage.tsx`, `RecipesPage.module.css`; test in
   `RecipesPage.test.tsx`).
3. **History rows showed a solid pink block instead of a readable state icon**
   (icon-only `StateTag` in solid variant; the glyph was nearly invisible).
   Fix: rows use `variant="line"` — glyph + state color, never color-only
   (`HistoryPage.tsx`; `StateTag` gained `data-state`; test in
   `HistoryPage.test.tsx`).

## Open

(none yet — next captures from the owner go here)
