import { describe, expect, it } from "vitest";

import { activePlace } from "./navPlaces";

describe("activePlace", () => {
  it.each([
    ["/app", "Overview"],
    ["/app/overview", "Overview"],
    ["/app/sessions/m:7", "Overview"],
    ["/app/sessions/hidden", "Overview"],
    ["/app/split", "Terminals"],
    ["/app/settings/machines", "Machines"],
    ["/app/settings", "Settings"],
    ["/app/settings/account", "Settings"],
    ["/app/history/", "History"],
    ["/app/recipes/run-tests", "Recipes"],
  ])("%s belongs to %s", (path, label) => {
    expect(activePlace(path)?.label).toBe(label);
  });

  it("marks nothing for a page outside the main places", () => {
    expect(activePlace("/app/settingsfoo")).toBeUndefined();
  });
});
