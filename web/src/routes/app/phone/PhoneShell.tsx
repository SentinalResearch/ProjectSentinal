import { useCallback, useMemo, useRef, useState } from "react";
import { Outlet } from "react-router-dom";

import { PromptBar } from "../shell/PromptBar";
import type { PromptSuggestion } from "../shell/promptGrammar";
import { ShellContext, type ShellValue } from "../shell/ShellContext";
import { usePromptActions } from "../shell/usePromptActions";
import { IslandBackContext, type IslandBack } from "./islandBack";
import { MobileIsland } from "./MobileIsland";
import styles from "./phone.module.css";

/**
 * The glass phone shell (MOBILE_GLASS_SPEC §1): an island on top, a prompt
 * pill at the bottom, no tab bar, menu, or sidebar. Detail screens put ‹ back
 * in the island; the prompt does everything the menu used to.
 */
export function PhoneShell() {
  const [back, setBack] = useState<IslandBack | null>(null);
  const [prompt, setPrompt] = useState<{ text: string } | null>(null);
  const { perform, dialogs } = usePromptActions();
  const pill = useRef<HTMLButtonElement>(null);

  const openPrompt = useCallback((prefill = "") => setPrompt({ text: prefill }), []);
  const shell = useMemo<ShellValue>(() => ({ openPrompt }), [openPrompt]);

  function choose(suggestion: PromptSuggestion, options: { split: boolean }) {
    setPrompt(null);
    if (perform(suggestion, options) === "keep-focus") pill.current?.focus();
  }

  return (
    <ShellContext.Provider value={shell}>
      <IslandBackContext.Provider value={setBack}>
        <div className={styles.shell}>
          <MobileIsland back={back} />
          <main className={styles.main} inert={prompt ? true : undefined}>
            <Outlet />
          </main>
          <button ref={pill} type="button" className={styles.promptPill} onClick={() => openPrompt()} aria-label="Open the prompt">
            <span className={styles.promptCaret} aria-hidden="true">
              ❯
            </span>
            <span className={styles.promptPlaceholder}>go, run, find, y / n…</span>
          </button>
          {prompt && (
            <PromptBar
              initialText={prompt.text}
              onClose={() => {
                setPrompt(null);
                pill.current?.focus();
              }}
              onChoose={choose}
            />
          )}
          {dialogs}
        </div>
      </IslandBackContext.Provider>
    </ShellContext.Provider>
  );
}
