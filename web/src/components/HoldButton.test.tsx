import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { HOLD_MS, HoldButton } from "./HoldButton";

describe("hold to send (MOBILE_GLASS_SPEC §5)", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("fires only after a 600 ms hold; letting go early or a plain click does nothing", () => {
    const held = vi.fn();
    render(<HoldButton onHeld={held}>Hold to send ^C</HoldButton>);
    const button = screen.getByRole("button", { name: "Hold to send ^C" });

    fireEvent.click(button);
    fireEvent.pointerDown(button);
    act(() => vi.advanceTimersByTime(HOLD_MS - 100));
    fireEvent.pointerUp(button);
    act(() => vi.advanceTimersByTime(1_000));
    expect(held).not.toHaveBeenCalled();

    fireEvent.pointerDown(button);
    act(() => vi.advanceTimersByTime(HOLD_MS));
    expect(held).toHaveBeenCalledTimes(1);
  });

  it("works by holding Enter or Space too", () => {
    const held = vi.fn();
    render(<HoldButton onHeld={held}>Hold</HoldButton>);
    const button = screen.getByRole("button", { name: "Hold" });
    fireEvent.keyDown(button, { key: " " });
    act(() => vi.advanceTimersByTime(HOLD_MS));
    expect(held).toHaveBeenCalledTimes(1);
  });
});
