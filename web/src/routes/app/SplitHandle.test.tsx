import { fireEvent, render, screen } from "@testing-library/react";
import { createRef } from "react";
import { describe, expect, it, vi } from "vitest";

import { clampSplit, SplitHandle, SPLIT_DEFAULT, SPLIT_MAX, SPLIT_MIN } from "./SplitHandle";

describe("clampSplit", () => {
  it("keeps the terminal between its limits", () => {
    expect(clampSplit(10)).toBe(SPLIT_MIN);
    expect(clampSplit(99)).toBe(SPLIT_MAX);
    expect(clampSplit(42.37)).toBe(42.4);
    expect(clampSplit(Number.NaN)).toBe(SPLIT_DEFAULT);
  });
});

describe("SplitHandle", () => {
  function renderHandle(split = 50) {
    const onChange = vi.fn();
    render(<SplitHandle container={createRef<HTMLElement>()} split={split} onChange={onChange} />);
    return { onChange, handle: screen.getByRole("separator", { name: "Resize the terminal" }) };
  }

  it("moves with the arrow keys and jumps to the ends with Home / End", () => {
    const { onChange, handle } = renderHandle(50);
    expect(handle).toHaveAttribute("aria-valuenow", "50");
    fireEvent.keyDown(handle, { key: "ArrowRight" });
    expect(onChange).toHaveBeenLastCalledWith(52);
    fireEvent.keyDown(handle, { key: "ArrowLeft" });
    expect(onChange).toHaveBeenLastCalledWith(48);
    fireEvent.keyDown(handle, { key: "Home" });
    expect(onChange).toHaveBeenLastCalledWith(SPLIT_MIN);
    fireEvent.keyDown(handle, { key: "End" });
    expect(onChange).toHaveBeenLastCalledWith(SPLIT_MAX);
  });

  it("goes back to half and half on double-click", () => {
    const { onChange, handle } = renderHandle(70);
    fireEvent.doubleClick(handle);
    expect(onChange).toHaveBeenLastCalledWith(SPLIT_DEFAULT);
  });
});
