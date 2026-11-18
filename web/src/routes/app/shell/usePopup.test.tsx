import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { InboxItem } from "../../../data/lifecycle";
import type { Session } from "../../../data/types";
import { usePopup } from "./usePopup";

afterEach(() => vi.useRealTimers());

function item(id: string, unread = true): InboxItem {
  const session = { id, machine: "m", name: id, state: "FAILED" } as Session;
  return { id: `${id}:FAILED:0`, session, group: "needs", unread };
}

describe("usePopup (new notifications pop, then tuck away)", () => {
  it("shows only the most urgent on load, hides it after the time, then shows each new one", () => {
    vi.useFakeTimers();
    let items = [item("a"), item("b")];
    const { result, rerender } = renderHook(({ paused }) => usePopup(items, true, paused, 6000), { initialProps: { paused: false } });
    expect(result.current.current?.id).toBe("a:FAILED:0");
    act(() => vi.advanceTimersByTime(6000));
    expect(result.current.current).toBeUndefined();
    items = [item("c"), ...items];
    rerender({ paused: false });
    expect(result.current.current?.id).toBe("c:FAILED:0");
  });

  it("holds while hovered, and × hides it at once", () => {
    vi.useFakeTimers();
    const items = [item("a")];
    const { result, rerender } = renderHook(({ paused }) => usePopup(items, true, paused, 6000), { initialProps: { paused: true } });
    act(() => vi.advanceTimersByTime(20000));
    expect(result.current.current?.id).toBe("a:FAILED:0");
    rerender({ paused: false });
    act(() => result.current.dismiss("a:FAILED:0"));
    expect(result.current.current).toBeUndefined();
  });
});
