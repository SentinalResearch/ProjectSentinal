import { useCallback, useState } from "react";

import { dataAdapter } from "../../data";

/** Hide, restore, and delete a Session, keeping refusals visible instead of dropping them. */
export function useSessionRemoval() {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const run = useCallback(async (action: () => Promise<void>): Promise<boolean> => {
    setBusy(true);
    setError(null);
    try {
      await action();
      return true;
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "That did not work. Try again.");
      return false;
    } finally {
      setBusy(false);
    }
  }, []);

  return {
    error,
    busy,
    hide: (ids: string[]) => run(async () => {
      for (const id of ids) await dataAdapter.dismissSession(id);
    }),
    restore: (ids: string[]) => run(async () => {
      for (const id of ids) await dataAdapter.restoreSession(id);
    }),
    remove: (ids: string[]) => run(async () => {
      for (const id of ids) await dataAdapter.deleteSession(id);
    }),
  };
}
