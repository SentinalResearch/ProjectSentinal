import { createContext, useContext } from "react";

/** What any page under the glass shell can ask of it. */
export interface ShellValue {
  /** Open the prompt, optionally prefilled (e.g. "go "). */
  openPrompt(prefill?: string): void;
}

export const ShellContext = createContext<ShellValue | null>(null);

/** The glass shell's controls, or null under the sidebar or phone shells. */
export function useShell(): ShellValue | null {
  return useContext(ShellContext);
}
