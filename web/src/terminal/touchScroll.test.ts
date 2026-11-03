import { describe, expect, it, vi } from "vitest";

import { attachTouchScroll, linesFor, type ScrollableTerminal } from "./touchScroll";

function touch(host: HTMLElement, type: string, y: number, at: number) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "touches", { value: type === "touchend" ? [] : [{ clientY: y }] });
  Object.defineProperty(event, "timeStamp", { value: at });
  host.dispatchEvent(event);
  return event;
}

function setup(type: "normal" | "alternate" = "normal") {
  const host = document.createElement("div");
  Object.defineProperty(host, "clientHeight", { value: 200 });
  const terminal: ScrollableTerminal = { rows: 10, scrollLines: vi.fn(), buffer: { active: { type } } };
  const sendKeys = vi.fn();
  const detach = attachTouchScroll(host, terminal, { sendKeys });
  return { host, terminal, sendKeys, detach };
}

describe("terminal touch scrolling", () => {
  it("counts whole lines and carries the rest", () => {
    expect(linesFor(45, 20)).toBe(2);
    expect(linesFor(-45, 20)).toBe(-2);
    expect(linesFor(10, 0)).toBe(0);
  });

  it("a drag down shows older lines and is not passed to the page", () => {
    const { host, terminal } = setup();
    touch(host, "touchstart", 100, 0);
    touch(host, "touchmove", 104, 400); // inside the slop: still a tap
    expect(terminal.scrollLines).not.toHaveBeenCalled();
    const move = touch(host, "touchmove", 164, 800);
    expect(move.defaultPrevented).toBe(true);
    // 60 px at 20 px a line = 3 lines up.
    expect(terminal.scrollLines).toHaveBeenCalledWith(-3);
    touch(host, "touchend", 164, 900);
  });

  it("a tap is left alone", () => {
    const { host, terminal } = setup();
    touch(host, "touchstart", 100, 0);
    const end = touch(host, "touchend", 100, 50);
    expect(end.defaultPrevented).toBe(false);
    expect(terminal.scrollLines).not.toHaveBeenCalled();
  });

  it("a full-screen program gets arrow keys instead", () => {
    const { host, terminal, sendKeys } = setup("alternate");
    touch(host, "touchstart", 100, 0);
    touch(host, "touchmove", 60, 800); // 40 px up = 2 lines down
    expect(sendKeys).toHaveBeenCalledWith("\x1b[B\x1b[B");
    expect(terminal.scrollLines).not.toHaveBeenCalled();
  });

  it("detaching stops listening", () => {
    const { host, terminal, detach } = setup();
    detach();
    touch(host, "touchstart", 100, 0);
    touch(host, "touchmove", 200, 800);
    expect(terminal.scrollLines).not.toHaveBeenCalled();
  });
});
