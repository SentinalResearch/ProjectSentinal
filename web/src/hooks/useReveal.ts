import { useEffect, useRef, useState, type RefObject } from "react";

function prefersReducedMotion(): boolean {
  return typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches === true;
}

/**
 * Fires once when `ref`'s element crosses into the top `fraction` of `root`'s
 * viewport (e.g. fraction 0.75 == the prototype's "top < vh * .75" trigger).
 * Implemented as an IntersectionObserver whose root is shrunk from the bottom
 * by `(1 - fraction) * 100%`, which is exactly equivalent to that top-position
 * check. Disconnects after the first trigger (each reveal runs once). With
 * prefers-reduced-motion, returns true immediately and never observes.
 */
export function useReveal(ref: RefObject<Element | null>, root: RefObject<Element | null>, fraction = 0.75): boolean {
  const [seen, setSeen] = useState(prefersReducedMotion);

  useEffect(() => {
    if (seen) return;
    const el = ref.current;
    if (!el) return;

    const rootMarginBottom = -Math.round((1 - fraction) * 100);
    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            setSeen(true);
            observer.disconnect();
          }
        }
      },
      { root: root.current, rootMargin: `0px 0px ${rootMarginBottom}% 0px`, threshold: 0 },
    );
    observer.observe(el);
    return () => observer.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [seen, fraction]);

  return seen;
}

export { prefersReducedMotion };
