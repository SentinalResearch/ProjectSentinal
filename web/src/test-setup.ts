import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

afterEach(cleanup);

// jsdom doesn't implement matchMedia; xterm.js (TerminalPane) and prefers-reduced-motion
// checks both need it. Minimal polyfill, not a real media-query engine.
if (!window.matchMedia) {
  window.matchMedia = (query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: () => {},
    removeListener: () => {},
    addEventListener: () => {},
    removeEventListener: () => {},
    dispatchEvent: () => false,
  });
}

// jsdom has no ResizeObserver either — TerminalPane observes its host element to re-fit.
if (!("ResizeObserver" in window)) {
  class NoopResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
  // @ts-expect-error -- test-only polyfill, not a spec-complete ResizeObserver
  window.ResizeObserver = NoopResizeObserver;
}
