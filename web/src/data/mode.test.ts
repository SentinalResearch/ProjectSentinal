import { describe, expect, it } from "vitest";

import { selectDataMode } from "./mode";

describe("selectDataMode", () => {
  it("uses the real daemon when no mode is configured", () => {
    expect(selectDataMode(undefined)).toBe("api");
  });

  it("enables fixtures only for the explicit mock setting", () => {
    expect(selectDataMode("mock")).toBe("mock");
    expect(selectDataMode("api")).toBe("api");
    expect(selectDataMode("typo")).toBe("api");
  });
});
