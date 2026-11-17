import { useCallback, useRef, useState, type KeyboardEvent, type PointerEvent, type RefObject } from "react";

import styles from "./SplitHandle.module.css";

const STORAGE_KEY = "sentinal.sessionSplit";
/** The terminal's share of the Session page, in percent. */
export const SPLIT_DEFAULT = 50;
export const SPLIT_MIN = 30;
export const SPLIT_MAX = 75;
const STEP = 2;

/** Keep a split inside the allowed range (NaN falls back to the default). */
export function clampSplit(value: number): number {
  if (!Number.isFinite(value)) return SPLIT_DEFAULT;
  return Math.round(Math.min(SPLIT_MAX, Math.max(SPLIT_MIN, value)) * 10) / 10;
}

function readSplit(): number {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    return stored === null ? SPLIT_DEFAULT : clampSplit(Number(stored));
  } catch {
    return SPLIT_DEFAULT;
  }
}

/** The terminal's width on the Session page, remembered in this browser. */
export function useSessionSplit(): [number, (next: number) => void] {
  const [split, setSplitState] = useState(readSplit);
  const setSplit = useCallback((next: number) => {
    const value = clampSplit(next);
    setSplitState(value);
    try {
      localStorage.setItem(STORAGE_KEY, String(value));
    } catch {
      // Storage may be unavailable; the split still applies for now.
    }
  }, []);
  return [split, setSplit];
}

/**
 * The divider between the terminal and the details: drag it, use the arrow
 * keys when focused, or double-click to go back to half and half.
 */
export function SplitHandle({
  container,
  split,
  onChange,
}: {
  /** The element the split is measured across. */
  container: RefObject<HTMLElement | null>;
  split: number;
  onChange: (next: number) => void;
}) {
  const dragging = useRef(false);

  const toPercent = (clientX: number) => {
    const box = container.current?.getBoundingClientRect();
    if (!box || box.width === 0) return split;
    return ((clientX - box.left) / box.width) * 100;
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    dragging.current = true;
    event.currentTarget.setPointerCapture(event.pointerId);
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  };
  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    if (dragging.current) onChange(toPercent(event.clientX));
  };
  const stop = (event: PointerEvent<HTMLDivElement>) => {
    if (!dragging.current) return;
    dragging.current = false;
    event.currentTarget.releasePointerCapture(event.pointerId);
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const moves: Record<string, number> = {
      ArrowLeft: split - STEP,
      ArrowRight: split + STEP,
      Home: SPLIT_MIN,
      End: SPLIT_MAX,
    };
    if (event.key in moves) {
      event.preventDefault();
      onChange(moves[event.key]);
    }
  };

  return (
    <div
      className={styles.handle}
      style={{ left: `${split}%` }}
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the terminal"
      aria-valuenow={Math.round(split)}
      aria-valuemin={SPLIT_MIN}
      aria-valuemax={SPLIT_MAX}
      tabIndex={0}
      title="Drag to resize · double-click for half and half"
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={stop}
      onPointerCancel={stop}
      onDoubleClick={() => onChange(SPLIT_DEFAULT)}
      onKeyDown={onKeyDown}
    >
      <span className={styles.grip} aria-hidden="true" />
    </div>
  );
}
