import { useSyncExternalStore } from "react";

import styles from "./LearnTip.module.css";

/**
 * The learning curve (UX pass): a new user is shown the two ways to move
 * around: the places menu for browsing, and search for speed. It goes away
 * once dismissed or once search has been used a few times.
 */

const KEY = "sentinal.learn";
const USES_TO_HIDE = 3;

interface LearnState {
  dismissed: boolean;
  searches: number;
}

const listeners = new Set<() => void>();
let cached: LearnState | null = null;

function read(): LearnState {
  if (cached) return cached;
  try {
    const parsed = JSON.parse(window.localStorage.getItem(KEY) ?? "{}") as Partial<LearnState>;
    cached = { dismissed: parsed.dismissed === true, searches: Number(parsed.searches) || 0 };
  } catch {
    cached = { dismissed: false, searches: 0 };
  }
  return cached;
}

function write(next: LearnState) {
  cached = next;
  try {
    window.localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // Private windows: this page only.
  }
  for (const listener of listeners) listener();
}

/** Call whenever search opens; after a few, the tip has done its job. */
export function noteSearchUsed() {
  const state = read();
  if (state.searches < USES_TO_HIDE) write({ ...state, searches: state.searches + 1 });
}

export function resetLearning() {
  cached = null;
  try {
    window.localStorage.removeItem(KEY);
  } catch {
    // ignore
  }
  for (const listener of listeners) listener();
}

function useLearning(): LearnState {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    read,
    read,
  );
}

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

export function LearnTip({ phone = false, onTry }: { phone?: boolean; onTry?: () => void }) {
  const state = useLearning();
  if (state.dismissed || state.searches >= USES_TO_HIDE) return null;
  return (
    <aside className={styles.tip} aria-label="How to get around">
      <p className={styles.text}>
        {phone ? (
          <>
            Tap the page name in the island to move around. Faster: tap the search pill and type{" "}
            <span className={styles.words}>go · run · find · y · end · inbox</span>.
          </>
        ) : (
          <>
            Browse with the page name in the island. Faster: press <kbd className={styles.key}>/</kbd> or{" "}
            <kbd className={styles.key}>{IS_MAC ? "⌘K" : "Ctrl K"}</kbd> and type{" "}
            <span className={styles.words}>go · run · find · y · end · inbox</span>.
          </>
        )}
      </p>
      <span className={styles.actions}>
        {onTry && (
          <button type="button" className={styles.try} onClick={onTry}>
            Try search
          </button>
        )}
        <button type="button" className={styles.dismiss} onClick={() => write({ ...read(), dismissed: true })}>
          Got it
        </button>
      </span>
    </aside>
  );
}
