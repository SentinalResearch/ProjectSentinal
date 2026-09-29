/**
 * Feature flags for UI that is in the brief but not fully designed or backed yet
 * (see design_handoff_sentinal/AGENT_PROMPT.md "Mock now, build for real later").
 * All false by default; true only when running against the mock adapter, so the
 * placeholder UI is visible while developing and absent once a real API lands.
 */
import { isMockData } from "./data/mode";

export const flags = {
  phoneScreens: isMockData,
  suggestionCard: isMockData,
  journalView: isMockData,
  askBox: isMockData,
  recipeEditor: false,
  pushNotifications: isMockData,
  multiBrowserSessions: isMockData,
} as const;

export type FlagName = keyof typeof flags;
