import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { LiveIsland } from "./LiveIsland";
import { PromptBar } from "./PromptBar";
import { SheetStage, type BackEntry } from "./SheetStage";
import { typingInto } from "./typing";
import { usePromptActions } from "./usePromptActions";
import type { PromptSuggestion } from "./promptGrammar";
import type { Session } from "../../../data/types";
import { ShellContext, type ShellValue } from "./ShellContext";
import styles from "./GlassShell.module.css";

/**
 * Desktop (≥ 768 px) shell: the Live island on top, the routed page below,
 * and the prompt on demand (GLASS_SPEC.md §4–5). The island's page name drops
 * down the main places for a new user; the prompt is the fast path. ⌘K / Ctrl-K and "/" open
 * the prompt; it only proposes, and this shell performs the chosen action
 * with the app's usual confirmations.
 */
export function GlassShell() {
  const { perform, dialogs } = usePromptActions();
  const [prompt, setPrompt] = useState<{ text: string } | null>(null);
  // A Kill… waiting for its confirmation in the island.
  const [interrupting, setInterrupting] = useState<Session | null>(null);
  // Whether that confirm ends the terminal or only sends Ctrl-C.
  const [ending, setEnding] = useState(false);
  // Pages you can go back to, newest first (shown in the island).
  const [back, setBack] = useState<BackEntry[]>([]);
  const returnFocus = useRef<HTMLElement | null>(null);

  const openPrompt = useCallback((prefill = "") => {
    returnFocus.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setPrompt({ text: prefill });
  }, []);

  const closePrompt = useCallback(() => {
    setPrompt(null);
    returnFocus.current?.focus?.();
  }, []);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const commandK = (event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k";
      const slash = event.key === "/" && !event.metaKey && !event.ctrlKey && !event.altKey && !typingInto(event.target);
      if (commandK || slash) {
        event.preventDefault();
        openPrompt();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [openPrompt]);

  function choose(suggestion: PromptSuggestion, options: { split: boolean }) {
    setPrompt(null);
    if (perform(suggestion, options) === "keep-focus") returnFocus.current?.focus?.();
  }

  const value = useMemo<ShellValue>(
    () => ({
      openPrompt,
      requestInterrupt: (session) => {
        setEnding(false);
        setInterrupting(session);
      },
      requestEnd: (session) => {
        setEnding(true);
        setInterrupting(session);
      },
    }),
    [openPrompt],
  );

  return (
    <ShellContext.Provider value={value}>
      <div className={styles.shell}>
        <div className={styles.glow} aria-hidden="true" />
        <LiveIsland
          onOpenPrompt={openPrompt}
          interrupting={interrupting}
          ending={ending}
          onCloseInterrupt={() => setInterrupting(null)}
          back={back}
        />
        <main className={styles.main} id="main" inert={prompt ? true : undefined}>
          <SheetStage onBackChange={setBack} />
        </main>
        {prompt && <PromptBar initialText={prompt.text} onClose={closePrompt} onChoose={choose} />}
        {dialogs}
      </div>
    </ShellContext.Provider>
  );
}
