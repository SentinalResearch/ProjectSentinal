import type { Terminal } from "@xterm/xterm";
import { useEffect, useState } from "react";

import { dataAdapter } from "../data";
import type { Session } from "../data/types";
import { writePriorParts, type PartMarker } from "./priorParts";

export interface PriorParts {
  /** Earlier parts, oldest first; empty for a terminal that never resumed. */
  parts: Session[];
  /** Earlier parts left out before `parts`. */
  offset: number;
  total: number;
  bytesPerPart: number;
}

/**
 * Draw the earlier parts into `terminal` once. `ready` turns true when they are
 * written (at once when there are none), so the live attach or the last part's
 * replay continues below them.
 */
export function usePriorParts(terminal: Terminal | null, latest: Session, prior: PriorParts | undefined) {
  const parts = prior?.parts ?? [];
  const key = `${parts.map((part) => part.id).join(",")}>${latest.id}`;
  const [done, setDone] = useState<{ key: string; markers: PartMarker[] } | null>(null);

  useEffect(() => {
    if (!terminal || !prior || prior.parts.length === 0) return;
    let cancelled = false;
    void writePriorParts(terminal, {
      parts: prior.parts,
      latest,
      offset: prior.offset,
      total: prior.total,
      maxBytes: prior.bytesPerPart,
      replay: (id, onChunk, options) => dataAdapter.replayOutput(id, onChunk, options),
      cancelled: () => cancelled,
    })
      .catch((error: unknown) => {
        // Never leave the live terminal waiting on its history.
        console.warn("earlier parts of this terminal could not be drawn", error);
        return [];
      })
      .then((markers) => {
        if (!cancelled) setDone({ key, markers });
      });
    return () => {
      cancelled = true;
    };
    // `key` names the parts; the Session objects change on every update.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [terminal, key]);

  const ready = parts.length === 0 || done?.key === key;
  return { ready, markers: done?.key === key ? done.markers : [] };
}
