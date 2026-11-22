import { describe, expect, it } from "vitest";

import { BASE_FONT, MIN_FONT, baseCells, fitFont, sizedForOther } from "./sessionSize";

describe("session size (T3)", () => {
  it("keeps the base font when the Session fits", () => {
    expect(fitFont(80, 120)).toBe(BASE_FONT);
    expect(fitFont(120, 120)).toBe(BASE_FONT);
  });

  it("shrinks the font to fit the width, in half pixels", () => {
    // 132 columns into a box of 100 at 13 px (one spare): 9.75 → 9.5.
    expect(fitFont(132, 100)).toBe(9.5);
    expect(fitFont(132, 120)).toBe(11.5);
  });

  it("never goes below the smallest readable font; the view pans instead", () => {
    expect(fitFont(300, 44)).toBe(MIN_FONT);
  });

  it("counts a box at the base font from what fits at a smaller one", () => {
    expect(baseCells(132, 40, 9.75)).toEqual({ cols: 99, rows: 30 });
    expect(baseCells(80, 24, BASE_FONT)).toEqual({ cols: 80, rows: 24 });
  });

  it("offers this viewer the size only when another set it and it differs", () => {
    const box = { cols: 100, rows: 30 };
    expect(sizedForOther(null, box)).toBe(false);
    expect(sizedForOther({ cols: 132, rows: 40, owner: true }, box)).toBe(false);
    expect(sizedForOther({ cols: 100, rows: 30, owner: false }, box)).toBe(false);
    expect(sizedForOther({ cols: 132, rows: 40, owner: false }, box)).toBe(true);
  });
});
