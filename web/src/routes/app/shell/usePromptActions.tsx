import { useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";

import { useAuth } from "../../../context/AuthContext";
import { useTheme } from "../../../context/ThemeContext";
import { answerPrompt } from "../../../data/actions";
import { isMockData } from "../../../data/mode";
import { useLiveData } from "../../../data/LiveDataContext";
import { InterruptConfirm } from "../InterruptConfirm";
import { useNewSession } from "../NewSession";
import type { PromptSuggestion } from "./promptGrammar";

/**
 * Performs what the prompt proposes, with the app's usual confirmations; the
 * same on the desktop and phone shells. Render `dialogs` once in the shell.
 */
export function usePromptActions(): {
  perform: (suggestion: PromptSuggestion, options: { split: boolean }) => "done" | "keep-focus";
  dialogs: ReactNode;
} {
  const navigate = useNavigate();
  const { machines, terminals } = useLiveData();
  const { toggleTheme } = useTheme();
  const { logout } = useAuth();
  const newSession = useNewSession((id) => `/app/sessions/${id}`);
  const [interrupting, setInterrupting] = useState<string | null>(null);

  function perform(suggestion: PromptSuggestion, { split }: { split: boolean }): "done" | "keep-focus" {
    if (split && suggestion.sessionId) {
      navigate(`/app/split?focus=${encodeURIComponent(suggestion.sessionId)}`);
      return "done";
    }
    const action = suggestion.action;
    switch (action.kind) {
      case "navigate":
        navigate(action.to);
        return "done";
      case "answer":
        void answerPrompt(action.sessionId, action.key);
        return "keep-focus";
      case "kill":
        setInterrupting(action.sessionId);
        return "done";
      case "newSession":
        if (isMockData) navigate("/app/recipes");
        else newSession.start(null);
        return "done";
      case "toggleTheme":
        toggleTheme();
        return "keep-focus";
      case "signOut":
        void logout();
        return "done";
    }
  }

  const target = interrupting ? terminals.find((session) => session.id === interrupting) : undefined;
  const targetMachine = target ? machines.find((machine) => machine.id === target.machine) : undefined;
  const dialogs = (
    <>
      <InterruptConfirm
        session={target}
        machineName={targetMachine?.name ?? target?.machine ?? ""}
        onClose={() => setInterrupting(null)}
      />
      {newSession.chooser}
    </>
  );
  return { perform, dialogs };
}
