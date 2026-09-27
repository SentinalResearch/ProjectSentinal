/**
 * Feature flags for UI that is in the brief but not fully designed or backed yet
 * (see design_handoff_sentinal/AGENT_PROMPT.md "Mock now, build for real later").
 * All false by default; true only when running against the mock adapter, so the
 * placeholder UI is visible while developing and absent once a real API lands.
 */
const mockMode = (import.meta.env.VITE_DATA ?? "mock") === "mock";

export const flags = {
  phoneScreens: mockMode,
  suggestionCard: mockMode,
  journalView: mockMode,
  askBox: mockMode,
  recipeEditor: false,
  pushNotifications: mockMode,
  multiBrowserSessions: mockMode,
} as const;

export type FlagName = keyof typeof flags;
