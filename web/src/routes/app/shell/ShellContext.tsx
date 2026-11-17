import { createContext, useContext } from "react";

import type { Session } from "../../../data/types";

/** What any page under the glass shell can ask of it. */
export interface ShellValue {
  /** Open the prompt, optionally prefilled (e.g. "go "). */
  openPrompt(prefill?: string): void;
  /** Ask, in the island, before sending Ctrl-C to a Session. */
  requestInterrupt?(session: Session): void;
  /** Ask, in the island, before ending a terminal: Ctrl-C, then close its shell. */
  requestEnd?(session: Session): void;
}

export const ShellContext = createContext<ShellValue | null>(null);

/** The glass shell's controls, or null under the sidebar or phone shells. */
export function useShell(): ShellValue | null {
  return useContext(ShellContext);
}
