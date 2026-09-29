export type SettingsTabKey = "machines" | "notifications" | "account" | "simulation";

export interface SettingsTab {
  key: SettingsTabKey;
  label: string;
}

const BASE_TABS: readonly SettingsTab[] = [
  { key: "machines", label: "Machines & tokens" },
  { key: "notifications", label: "Notifications" },
  { key: "account", label: "Account" },
];

/** Return Settings tabs for the selected build-time data source. */
export function settingsTabs(mockData: boolean): readonly SettingsTab[] {
  return mockData ? [...BASE_TABS, { key: "simulation", label: "Simulation" }] : BASE_TABS;
}
