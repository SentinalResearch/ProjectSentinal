/**
 * Test helper: make `matchMedia` answer width queries for a given viewport,
 * so phone (< 768 px) and desktop layouts can both be exercised in jsdom.
 */
const original = window.matchMedia;

export function setViewport(width: number, options: { prefersDark?: boolean; height?: number; coarse?: boolean } = {}): void {
  const height = options.height ?? 844;
  Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: width });
  window.matchMedia = (query: string) => {
    // Comma-separated alternatives; each may combine width, height, and pointer tests.
    const matches = query.split(",").some((part) => {
      if (part.includes("prefers-color-scheme: dark")) return options.prefersDark ?? false;
      const maxWidth = /max-width:\s*(\d+)px/.exec(part);
      const minWidth = /min-width:\s*(\d+)px/.exec(part);
      const maxHeight = /max-height:\s*(\d+)px/.exec(part);
      if (!maxWidth && !minWidth && !maxHeight) return false;
      return (!maxWidth || width <= Number(maxWidth[1]))
        && (!minWidth || width >= Number(minWidth[1]))
        && (!maxHeight || height <= Number(maxHeight[1]))
        && (!part.includes("pointer: coarse") || (options.coarse ?? false));
    });
    return {
      matches,
      media: query,
      onchange: null,
      addListener: () => {},
      removeListener: () => {},
      addEventListener: () => {},
      removeEventListener: () => {},
      dispatchEvent: () => false,
    };
  };
}

export function resetViewport(): void {
  window.matchMedia = original;
  Object.defineProperty(window, "innerWidth", { writable: true, configurable: true, value: 1024 });
}
