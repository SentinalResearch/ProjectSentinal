import { useSyncExternalStore } from "react";

/** Widest viewport (px) that uses the phone layout (MOBILE_SPEC.md → Breakpoint). */
export const PHONE_MAX_WIDTH = 767;
/** A touch screen this short is a phone in landscape (844×390), not a desktop. */
export const PHONE_LANDSCAPE_MAX_HEIGHT = 500;
export const PHONE_QUERY = `(max-width: ${PHONE_MAX_WIDTH}px), (max-height: ${PHONE_LANDSCAPE_MAX_HEIGHT}px) and (pointer: coarse)`;

function subscribe(onChange: () => void): () => void {
  const media = window.matchMedia(PHONE_QUERY);
  media.addEventListener?.("change", onChange);
  window.addEventListener("resize", onChange);
  return () => {
    media.removeEventListener?.("change", onChange);
    window.removeEventListener("resize", onChange);
  };
}

/**
 * True below 768 px, or for a phone turned to landscape: render the phone
 * component tree instead of the desktop one, so rotating never swaps layouts.
 */
export function useIsPhone(): boolean {
  return useSyncExternalStore(subscribe, () => window.matchMedia(PHONE_QUERY).matches, () => false);
}
