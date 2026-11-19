import { act, renderHook } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

const { dismissSession } = vi.hoisted(() => ({ dismissSession: vi.fn(async (_id: string) => undefined) }));
vi.mock("../../data", () => ({ dataAdapter: { dismissSession, restoreSession: vi.fn(), deleteSession: vi.fn() } }));
vi.mock("../../data/LiveDataContext", () => ({
  useOptionalLiveData: () => ({
    machines: [
      { id: "old", name: "sentinal-research", daemonVersion: "2.10.0" },
      { id: "new", name: "pop-os", daemonVersion: "2.11.0" },
    ],
    sessions: [
      { id: "old:3", machine: "old", state: "INTERRUPTED" },
      { id: "new:5", machine: "new", state: "INTERRUPTED" },
    ],
  }),
}));

import { useSessionRemoval } from "./useSessionRemoval";

describe("useSessionRemoval", () => {
  it("hides what each Machine supports and explains the rest without asking it", async () => {
    const { result } = renderHook(() => useSessionRemoval());

    await act(async () => {
      await result.current.hide(["old:3", "new:5"]);
    });

    expect(dismissSession).toHaveBeenCalledTimes(1);
    expect(dismissSession).toHaveBeenCalledWith("new:5");
    expect(result.current.error).toBe("sentinal-research runs Sentinal 2.10.0. Hiding a stopped Session needs 2.11.");
  });

  it("turns a raw refusal into words", async () => {
    dismissSession.mockRejectedValueOnce(
      new Error("daemon request failed: invalid JSON: unknown variant `DISMISS_SESSION`, expected one of `ATTACH`"),
    );
    const { result } = renderHook(() => useSessionRemoval());

    await act(async () => {
      await result.current.hide(["new:5"]);
    });

    expect(result.current.error).toBe(
      "This Machine runs an older Sentinal that cannot hide or delete Sessions. Update it to 2.11.",
    );
  });
});
