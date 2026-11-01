import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate } from "react-router-dom";

import { useAuth } from "../../../context/AuthContext";
import { useTheme } from "../../../context/ThemeContext";
import { answerPrompt } from "../../../data/actions";
import { isMockData } from "../../../data/mode";
import { useLiveData } from "../../../data/LiveDataContext";
import { InterruptConfirm } from "../InterruptConfirm";
import { useNewSession } from "../NewSession";
import { LiveIsland } from "./LiveIsland";
import { PromptBar } from "./PromptBar";
import { SheetStage } from "./SheetStage";
import { typingInto } from "./typing";
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
  const navigate = useNavigate();
  const { machines, terminals } = useLiveData();
  const { toggleTheme } = useTheme();
  const { logout } = useAuth();
  const newSession = useNewSession((id) => `/app/sessions/${id}`);
  const [prompt, setPrompt] = useState<{ text: string } | null>(null);
  const [interrupting, setInterrupting] = useState<string | null>(null);
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

  function perform(suggestion: PromptSuggestion, { split }: { split: boolean }) {
    setPrompt(null);
    if (split && suggestion.sessionId) {
      navigate(`/app/split?focus=${encodeURIComponent(suggestion.sessionId)}`);
      return;
    }
    const action = suggestion.action;
    switch (action.kind) {
      case "navigate":
        navigate(action.to);
        return;
      case "answer":
        void answerPrompt(action.sessionId, action.key);
        returnFocus.current?.focus?.();
        return;
      case "kill":
        setInterrupting(action.sessionId);
        return;
      case "newSession":
        if (isMockData) navigate("/app/recipes");
        else newSession.start(null);
        return;
      case "toggleTheme":
        toggleTheme();
        returnFocus.current?.focus?.();
        return;
      case "signOut":
        void logout();
        return;
    }
  }

  const value = useMemo<ShellValue>(() => ({ openPrompt }), [openPrompt]);
  const target = interrupting ? terminals.find((session) => session.id === interrupting) : undefined;
  const targetMachine = target ? machines.find((machine) => machine.id === target.machine) : undefined;

  return (
    <ShellContext.Provider value={value}>
      <div className={styles.shell}>
        <div className={styles.glow} aria-hidden="true" />
        <LiveIsland onOpenPrompt={openPrompt} />
        <main className={styles.main} id="main" inert={prompt ? true : undefined}>
          <SheetStage />
        </main>
        {prompt && <PromptBar initialText={prompt.text} onClose={closePrompt} onChoose={perform} />}
        <InterruptConfirm
          session={target}
          machineName={targetMachine?.name ?? target?.machine ?? ""}
          onClose={() => setInterrupting(null)}
        />
        {newSession.chooser}
      </div>
    </ShellContext.Provider>
  );
}
