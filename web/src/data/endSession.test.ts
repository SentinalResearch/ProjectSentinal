import { afterEach, describe, expect, it, vi } from "vitest";

import { endSession } from "./actions";
import { dataAdapter } from "./index";

afterEach(() => vi.restoreAllMocks());

describe("endSession", () => {
  it("sends Ctrl-C, then closes the shell if it is still live", async () => {
    const signal = vi.spyOn(dataAdapter, "sendSignal").mockResolvedValue();
    await endSession("s1", () => true, () => Promise.resolve());
    expect(signal.mock.calls).toEqual([["s1", "SIGINT"], ["s1", "KILL"]]);
  });

  it("stops at Ctrl-C when the program ended", async () => {
    const signal = vi.spyOn(dataAdapter, "sendSignal").mockResolvedValue();
    await endSession("s1", () => false, () => Promise.resolve());
    expect(signal.mock.calls).toEqual([["s1", "SIGINT"]]);
  });
});
