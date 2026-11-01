import { createContext, useContext, type ReactNode } from "react";

/** What a detail screen tells the phone island: where back goes, and an optional trailing control. */
export interface IslandBack {
  parentLabel: string;
  parentPath: string;
  trailing?: ReactNode;
}

export const IslandBackContext = createContext<((back: IslandBack | null) => void) | null>(null);

/** The registrar a detail screen's BackBar uses; null outside the phone shell. */
export function useIslandBackRegistrar(): ((back: IslandBack | null) => void) | null {
  return useContext(IslandBackContext);
}
