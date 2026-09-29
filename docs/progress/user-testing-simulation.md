# User-testing simulation harness

## Tasks and proof

- [x] Define fixture-only scenarios without changing the production adapter —
  `web/src/data/mock/simulation.test.ts`.
- [x] Make playback pauseable, manually advanceable, and resettable —
  `web/src/data/mock/simulation.test.ts` with fake timers.
- [x] Add a Simulation Settings tab only in explicit mock mode —
  `web/src/routes/app/SettingsPage.test.tsx`.
- [x] Refresh shared Machine/Session state when a scenario changes —
  `web/src/data/LiveDataContext.test.tsx` or Settings integration coverage.
- [x] Add an exact user-research guide and launch command —
  `docs/USER_TESTING.md` and the launcher shell test.
- [x] Run `just check` and commit the isolated change.

## Open questions resolved for this slice

- Simulation never switches an API build to mock data. It exists only when
  `VITE_DATA=mock` was selected before Vite starts.
- Presets use subsets/controlled variations of committed fixtures. They do not
  claim the missing watcher, pairing, Recipe, or notification backends work.
- Authentication is not reset by scenario changes so a facilitator is not
  repeatedly sent through setup. Simulated product state is reset separately.
- Browser refresh preserves the selected scenario and playback speed for the
  current tab only (`sessionStorage`); a new browser tab starts from the mixed
  baseline.
