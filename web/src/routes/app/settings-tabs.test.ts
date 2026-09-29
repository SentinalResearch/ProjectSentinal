import { describe, expect, it } from "vitest";

import { settingsTabs } from "./settings-tabs";

describe("settingsTabs", () => {
  it("offers Simulation in explicit fixture mode", () => {
    expect(settingsTabs(true).map((tab) => tab.key)).toContain("simulation");
  });

  it("never offers Simulation in real API mode", () => {
    expect(settingsTabs(false).map((tab) => tab.key)).not.toContain("simulation");
  });
});
