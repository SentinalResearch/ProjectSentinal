import { useEffect, useRef } from "react";

import { dataAdapter } from "../../data";
import { autoClearDue, useLifecycle } from "../../data/lifecycle";
import { useLiveData } from "../../data/LiveDataContext";

const EVERY_MS = 60_000;

/**
 * Hides finished terminals once they are older than the auto-clear setting
 * (design L1). Hide only, never delete; runs while Sentinal is open here.
 */
export function AutoClear() {
  const { terminals } = useLiveData();
  const life = useLifecycle();
  const latest = useRef({ terminals, life });
  latest.current = { terminals, life };
  const tried = useRef(new Set<string>());

  useEffect(() => {
    const sweep = () => {
      const { terminals: now, life: settings } = latest.current;
      for (const id of autoClearDue(now, settings, Date.now())) {
        if (tried.current.has(id)) continue;
        tried.current.add(id);
        void dataAdapter.dismissSession(id).catch(() => undefined);
      }
    };
    sweep();
    const timer = setInterval(sweep, EVERY_MS);
    return () => clearInterval(timer);
  }, [life.autoClear]);
  return null;
}
