import { describe, expect, it } from "vitest";

import { APP_PAGES } from "../pages";
import { activePlace, NAV_PLACES } from "./navPlaces";

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

describe("every app route has a place (A2)", () => {
  // Every concrete route in APP_PAGES (skipping the catch-all) must resolve to
  // a NAV_PLACE, so the island's page menu always offers a way back.
  const concretePaths = APP_PAGES
    .filter((page) => !page.index && page.path !== "*" && page.path !== "more" && !page.path!.includes(":"))
    .map((page) => `/app/${page.path!.replace(/\/$/, "")}`);

  it.each(concretePaths)("%s has a visible place", (path) => {
    const place = activePlace(path);
    expect(place, `${path} has no place in NAV_PLACES`).toBeDefined();
    expect(NAV_PLACES.some((p) => p.label === place!.label)).toBe(true);
  });

  it("detail routes (with :params) belong to a place too", () => {
    for (const path of ["/app/sessions/m:7", "/app/sessions/m:7/terminal", "/app/projects/my-proj", "/app/recipes/run"]) {
      expect(activePlace(path), `${path} has no place`).toBeDefined();
    }
  });
});
