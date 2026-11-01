import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { LiveIsland } from "./LiveIsland";
import { PromptBar } from "./PromptBar";
import { SheetStage } from "./SheetStage";
import { typingInto } from "./typing";
import { usePromptActions } from "./usePromptActions";
import type { PromptSuggestion } from "./promptGrammar";
import { ShellContext, type ShellValue } from "./ShellContext";
import styles from "./GlassShell.module.css";

/**
 * Desktop (≥ 1024 px) shell: the Live island on top, the routed page below,
 * and the prompt on demand (GLASS_SPEC.md §4–5). ⌘K / Ctrl-K and "/" open
 * the prompt; it only proposes, and this shell performs the chosen action
 * with the app's usual confirmations.
 */
export function GlassShell() {
  const { perform, dialogs } = usePromptActions();
  const [prompt, setPrompt] = useState<{ text: string } | null>(null);
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

  const value = useMemo<ShellValue>(() => ({ openPrompt }), [openPrompt]);

  return (
    <ShellContext.Provider value={value}>
      <div className={styles.shell}>
        <div className={styles.glow} aria-hidden="true" />
        <LiveIsland onOpenPrompt={openPrompt} />
        <main className={styles.main} id="main" inert={prompt ? true : undefined}>
          <SheetStage />
        </main>
        {prompt && <PromptBar initialText={prompt.text} onClose={closePrompt} onChoose={choose} />}
        {dialogs}
      </div>
    </ShellContext.Provider>
  );
}
