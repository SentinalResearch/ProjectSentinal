import { useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";

import { useAuth } from "../../../context/AuthContext";
import { useTheme } from "../../../context/ThemeContext";
import { answerPrompt } from "../../../data/actions";
import { isMockData } from "../../../data/mode";
import { useLiveData } from "../../../data/LiveDataContext";
import { InterruptConfirm } from "../InterruptConfirm";
import { useNewSession } from "../NewSession";
import { useSessionRemoval } from "../useSessionRemoval";
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
  const removal = useSessionRemoval();
  const [interrupting, setInterrupting] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);

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
      case "hide":
        void removal.hide([action.sessionId]);
        return "keep-focus";
      case "restore":
        void removal.restore([action.sessionId]);
        return "keep-focus";
      case "delete":
        setDeleting(action.sessionId);
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
  const deletingSession = deleting ? terminals.find((session) => session.id === deleting) : undefined;
  const dialogs = (
    <>
      <InterruptConfirm
        session={target}
        machineName={targetMachine?.name ?? target?.machine ?? ""}
        onClose={() => setInterrupting(null)}
      />
      {deletingSession && (
        <div
          role="alertdialog"
          aria-modal="true"
          aria-label="Delete Session"
          style={{
            position: "fixed",
            inset: 0,
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            background: "rgba(0,0,0,0.5)",
            zIndex: 1000,
          }}
        >
          <div style={{ background: "var(--bg-1)", padding: "24px", borderRadius: "8px", maxWidth: "400px" }}>
            <h2>Delete {deletingSession.name}?</h2>
            <p>This permanently erases the Session and its output. This cannot be undone.</p>
            <div style={{ display: "flex", gap: "8px", justifyContent: "flex-end", marginTop: "16px" }}>
              <button type="button" onClick={() => setDeleting(null)}>
                Cancel
              </button>
              <button
                type="button"
                onClick={() => {
                  void removal.remove([deletingSession.id]);
                  setDeleting(null);
                }}
              >
                Delete permanently
              </button>
            </div>
          </div>
        </div>
      )}
      {newSession.chooser}
    </>
  );
  return { perform, dialogs };
}
