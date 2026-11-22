/**
 * Run `action` once `waitMs` after the last call (T1.2): a window drag or a
 * panel animation fires dozens of resize events, and each fit used to send
 * the PTY a new size and make the program redraw.
 */
export function trailing(action: () => void, waitMs: number): { call: () => void; cancel: () => void } {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return {
    call: () => {
      if (timer !== undefined) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = undefined;
        action();
      }, waitMs);
    },
    cancel: () => {
      if (timer !== undefined) clearTimeout(timer);
      timer = undefined;
    },
  };
}

/** One fit per this long while the terminal's box keeps changing. */
export const FIT_DEBOUNCE_MS = 100;

/**
 * Unicode 11 character widths (T1.1), so emoji and wide symbols take the
 * cells the program assumed. A proposed API in xterm 6.
 */
export function activateUnicode11(term: { unicode: { activeVersion: string }; loadAddon: (addon: never) => void }, addon: unknown): void {
  term.loadAddon(addon as never);
  term.unicode.activeVersion = "11";
}
