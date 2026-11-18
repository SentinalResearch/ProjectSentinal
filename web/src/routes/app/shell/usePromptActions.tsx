import { useRef, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";

import { ConfirmDialog } from "../../../components/ConfirmDialog";
import { useAuth } from "../../../context/AuthContext";
import { useTheme } from "../../../context/ThemeContext";
import { answerPrompt, endSession } from "../../../data/actions";
import { inboxItems, lifecycle } from "../../../data/lifecycle";
import { isMockData } from "../../../data/mode";
import { useLiveData } from "../../../data/LiveDataContext";
import { InterruptConfirm } from "../InterruptConfirm";
import { useNewSession } from "../NewSession";
import { useSessionRemoval } from "../useSessionRemoval";
import type { PromptSuggestion } from "./promptGrammar";

/** Asks whichever island is showing to open its inbox. */
export const OPEN_INBOX_EVENT = "sentinal:open-inbox";

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
  const latest = useRef(terminals);
  latest.current = terminals;
  const stillLive = (id: string) => {
    const state = latest.current.find((session) => session.id === id)?.state;
    return state !== undefined && !["COMPLETED", "FAILED", "INTERRUPTED"].includes(state);
  };
  const [interrupting, setInterrupting] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<string | null>(null);
  const [ending, setEnding] = useState<string | null>(null);

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
      case "end":
        setEnding(action.sessionId);
        return "done";
      case "openInbox":
        window.dispatchEvent(new Event(OPEN_INBOX_EVENT));
        return "done";
      case "markAllRead":
        lifecycle.markRead(inboxItems(terminals, lifecycle.get()).map((item) => item.id));
        return "keep-focus";
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
  const endingSession = ending ? terminals.find((session) => session.id === ending) : undefined;
  const dialogs = (
    <>
      <InterruptConfirm
        session={target}
        machineName={targetMachine?.name ?? target?.machine ?? ""}
        onClose={() => setInterrupting(null)}
      />
      <ConfirmDialog
        open={Boolean(deletingSession)}
        overline="DELETE PERMANENTLY"
        question={`Delete ${deletingSession?.name ?? ""}?`}
        consequence="This erases the Session and its saved output. It cannot be undone."
        confirmLabel="Delete permanently"
        focusCancel
        onConfirm={() => {
          if (deletingSession) void removal.remove([deletingSession.id]);
          setDeleting(null);
        }}
        onCancel={() => setDeleting(null)}
      />
      <ConfirmDialog
        open={Boolean(endingSession)}
        overline="END TERMINAL"
        question={`End ${endingSession?.name ?? ""}?`}
        consequence="Sends Ctrl-C, then closes the terminal if it is still open. Its output is kept and it moves to Done."
        confirmLabel="End"
        onConfirm={() => {
          const id = endingSession?.id;
          setEnding(null);
          if (id) void endSession(id, () => stillLive(id)).catch(() => undefined);
        }}
        onCancel={() => setEnding(null)}
      />
      {newSession.chooser}
    </>
  );
  return { perform, dialogs };
}
