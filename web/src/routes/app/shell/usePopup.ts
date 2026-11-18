import { useEffect, useState } from "react";

import type { InboxItem } from "../../../data/lifecycle";

/** How long a new notification stays dropped down under the island. */
export const POPUP_MS = 6_000;

/**
 * Which notification the island shows now (UX pass). Each new unread
 * notification drops down once for `ms`, then tucks away into the inbox (it
 * stays unread there); the next new one then shows. Hovering pauses the
 * timer. On first load only the most urgent one shows, not a backlog.
 */
export function usePopup(items: InboxItem[], ready: boolean, paused: boolean, ms: number = POPUP_MS) {
  const [shown, setShown] = useState<ReadonlySet<string> | null>(null);

  useEffect(() => {
    if (ready && shown === null) setShown(new Set(items.slice(1).map((item) => item.id)));
    // Only the first ready render seeds the backlog.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready]);

  const current = shown ? items.find((item) => item.unread && !shown.has(item.id)) : undefined;
  const dismiss = (id: string) => setShown((previous) => new Set(previous ?? []).add(id));

  useEffect(() => {
    if (!current || paused) return;
    const id = current.id;
    const timer = setTimeout(() => setShown((previous) => new Set(previous ?? []).add(id)), ms);
    return () => clearTimeout(timer);
  }, [current?.id, paused, ms]);

  return { current, dismiss };
}
