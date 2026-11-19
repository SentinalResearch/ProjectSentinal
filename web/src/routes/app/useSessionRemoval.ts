import { useCallback, useState } from "react";

import { dataAdapter } from "../../data";
import { useOptionalLiveData } from "../../data/LiveDataContext";
import { friendlyRemovalError, removalSupport, type RemovalSupport } from "../../data/removal";

type Ability = keyof Omit<RemovalSupport, "note">;

/**
 * Hide, restore, and delete Sessions, keeping refusals visible instead of
 * dropping them. A Session whose Machine runs a release that cannot do it is
 * not sent at all; the owner is told which Machine needs which version.
 */
export function useSessionRemoval() {
  const live = useOptionalLiveData();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const supportOf = useCallback(
    (id: string): RemovalSupport => {
      const session = live?.sessions.find((candidate) => candidate.id === id);
      if (!session) return { hide: true, restore: true, remove: true };
      return removalSupport(session, live?.machines.find((machine) => machine.id === session.machine));
    },
    [live],
  );

  const run = useCallback(
    async (ids: string[], ability: Ability, action: (id: string) => Promise<void>): Promise<boolean> => {
      setBusy(true);
      setError(null);
      const notes = new Set<string>();
      try {
        for (const id of ids) {
          const support = supportOf(id);
          if (!support[ability]) {
            if (support.note) notes.add(support.note);
            continue;
          }
          await action(id);
        }
        if (notes.size > 0) setError([...notes].join(" "));
        return notes.size === 0;
      } catch (failure) {
        setError(friendlyRemovalError(failure instanceof Error ? failure.message : "That did not work. Try again."));
        return false;
      } finally {
        setBusy(false);
      }
    },
    [supportOf],
  );

  return {
    error,
    busy,
    supportOf,
    hide: (ids: string[]) => run(ids, "hide", (id) => dataAdapter.dismissSession(id)),
    restore: (ids: string[]) => run(ids, "restore", (id) => dataAdapter.restoreSession(id)),
    remove: (ids: string[]) => run(ids, "remove", (id) => dataAdapter.deleteSession(id)),
  };
}
