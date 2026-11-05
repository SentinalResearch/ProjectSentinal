import { useState } from "react";
import { useNavigate } from "react-router-dom";

import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import type { Session } from "../../data/types";
import styles from "./SessionRemoveActions.module.css";
import { useSessionRemoval } from "./useSessionRemoval";

const FINISHED = new Set(["COMPLETED", "FAILED", "INTERRUPTED"]);

/** Hide or Restore, and Delete, for a finished Session. Nothing for a live one. */
export function SessionRemoveActions({ session }: { session: Session }) {
  const navigate = useNavigate();
  const removal = useSessionRemoval();
  const [confirming, setConfirming] = useState(false);
  if (!FINISHED.has(session.state)) return null;

  async function hideOrRestore() {
    if (session.dismissed) {
      await removal.restore([session.id]);
    } else if (await removal.hide([session.id])) {
      navigate("/app/overview");
    }
  }

  async function erase() {
    setConfirming(false);
    if (await removal.remove([session.id])) navigate("/app/overview", { replace: true });
  }

  return (
    <div className={styles.actions}>
      <Button type="button" disabled={removal.busy} onClick={() => void hideOrRestore()}>
        {session.dismissed ? "Restore" : "Hide"}
      </Button>
      <Button type="button" disabled={removal.busy} onClick={() => setConfirming(true)}>
        Delete…
      </Button>
      {removal.error && (
        <p className={styles.error} role="alert">
          {removal.error}
        </p>
      )}
      <ConfirmDialog
        open={confirming}
        overline="Delete Session"
        question={`Permanently delete ${session.name}?`}
        consequence="Its saved output, Commands, and Events are erased from this Machine, including any earlier generations of this terminal. This cannot be undone."
        confirmLabel="Delete"
        onConfirm={() => void erase()}
        onCancel={() => setConfirming(false)}
      />
    </div>
  );
}
