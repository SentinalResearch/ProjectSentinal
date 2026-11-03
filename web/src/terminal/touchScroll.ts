/**
 * One-finger scrolling for the terminal on touch screens. xterm's own viewport
 * scrolls poorly on iOS (and the drag often scrolls the page instead), so a
 * vertical drag here moves the scrollback a line at a time and keeps a short
 * fling. A tap still reaches xterm; only a drag past the slop is taken.
 * Full-screen programs (the alternate screen has no scrollback) get arrow keys
 * instead, the way desktop terminals scroll them.
 */

/** The part of xterm's Terminal this needs. */
export interface ScrollableTerminal {
  rows: number;
  scrollLines(amount: number): void;
  buffer: { active: { type: "normal" | "alternate" } };
}

export interface TouchScrollOptions {
  /** Arrow keys for a full-screen program; omitted when the terminal is read-only. */
  sendKeys?: (bytes: string) => void;
}

const SLOP_PX = 8;
const FRICTION = 0.95;
const MIN_FLING_PX_PER_MS = 0.3;

/** Whole lines to scroll for `pixels` of drag; the remainder carries over. */
export function linesFor(pixels: number, lineHeight: number): number {
  if (lineHeight <= 0) return 0;
  return Math.trunc(pixels / lineHeight);
}

export function attachTouchScroll(
  host: HTMLElement,
  terminal: ScrollableTerminal,
  options: TouchScrollOptions = {},
): () => void {
  let lastY = 0;
  let lastTime = 0;
  let travelled = 0;
  let pending = 0;
  let velocity = 0;
  let dragging = false;
  let frame = 0;

  const lineHeight = () => host.clientHeight / Math.max(1, terminal.rows);

  /** Positive `pixels` = finger moved down = show older lines. */
  function scrollBy(pixels: number) {
    pending += pixels;
    const lines = linesFor(pending, lineHeight());
    if (lines === 0) return;
    pending -= lines * lineHeight();
    if (terminal.buffer.active.type === "alternate") {
      options.sendKeys?.((lines > 0 ? "\x1b[A" : "\x1b[B").repeat(Math.abs(lines)));
    } else {
      terminal.scrollLines(-lines);
    }
  }

  function stopFling() {
    if (frame) cancelAnimationFrame(frame);
    frame = 0;
  }

  function onStart(event: TouchEvent) {
    stopFling();
    if (event.touches.length !== 1) return;
    lastY = event.touches[0].clientY;
    lastTime = event.timeStamp;
    travelled = 0;
    pending = 0;
    velocity = 0;
    dragging = false;
  }

  function onMove(event: TouchEvent) {
    if (event.touches.length !== 1) return;
    const y = event.touches[0].clientY;
    const dy = y - lastY;
    travelled += Math.abs(dy);
    if (!dragging && travelled < SLOP_PX) return;
    dragging = true;
    event.preventDefault();
    event.stopPropagation();
    const elapsed = Math.max(1, event.timeStamp - lastTime);
    velocity = dy / elapsed;
    lastY = y;
    lastTime = event.timeStamp;
    scrollBy(dy);
  }

  function onEnd(event: TouchEvent) {
    if (!dragging) return;
    event.preventDefault();
    dragging = false;
    if (Math.abs(velocity) < MIN_FLING_PX_PER_MS) return;
    let previous = performance.now();
    const step = (now: number) => {
      const elapsed = now - previous;
      previous = now;
      scrollBy(velocity * elapsed);
      velocity *= FRICTION ** (elapsed / 16);
      frame = Math.abs(velocity) < 0.05 ? 0 : requestAnimationFrame(step);
    };
    frame = requestAnimationFrame(step);
  }

  host.addEventListener("touchstart", onStart, { capture: true, passive: true });
  host.addEventListener("touchmove", onMove, { capture: true, passive: false });
  host.addEventListener("touchend", onEnd, { capture: true, passive: false });
  host.addEventListener("touchcancel", onEnd, { capture: true, passive: false });
  return () => {
    stopFling();
    host.removeEventListener("touchstart", onStart, { capture: true });
    host.removeEventListener("touchmove", onMove, { capture: true });
    host.removeEventListener("touchend", onEnd, { capture: true });
    host.removeEventListener("touchcancel", onEnd, { capture: true });
  };
}
