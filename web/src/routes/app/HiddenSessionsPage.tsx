import { useState } from "react";
import { Link } from "react-router-dom";

import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { StateTag } from "../../components/StateTag";
import { useLiveData } from "../../data/LiveDataContext";
import styles from "./HiddenSessionsPage.module.css";
import { useSessionRemoval } from "./useSessionRemoval";

/** Sessions the owner hid: still searchable, restorable, or deletable for good. */
export function HiddenSessionsPage() {
  const { hidden, machines } = useLiveData();
  const removal = useSessionRemoval();
  const [confirming, setConfirming] = useState(false);
  const machineName = (id: string) => machines.find((machine) => machine.id === id)?.name ?? id;

  return (
    <div className={styles.page}>
      <header className={styles.header}>
        <h1 className={styles.title}>Hidden Sessions</h1>
        {hidden.length > 0 && (
          <Button type="button" disabled={removal.busy} onClick={() => setConfirming(true)}>
            Delete all {hidden.length}…
          </Button>
        )}
      </header>
      {removal.error && (
        <p className={styles.error} role="alert">
          {removal.error}
        </p>
      )}
      {hidden.length === 0 ? (
        <p className={styles.empty}>No hidden Sessions. Hide a finished Session to clear it from your lists.</p>
      ) : (
        <ul className={styles.list}>
          {hidden.map((session) => (
            <li key={session.id} className={styles.row}>
              <StateTag state={session.state} size="sm" />
              <Link className={styles.name} to={`/app/sessions/${session.id}`}>
                {session.name}
              </Link>
              <span className={styles.machine}>{machineName(session.machine)}</span>
              <Button type="button" disabled={removal.busy} onClick={() => void removal.restore([session.id])}>
                Restore
              </Button>
            </li>
          ))}
        </ul>
      )}
      <ConfirmDialog
        open={confirming}
        overline="Delete hidden Sessions"
        question={`Permanently delete ${hidden.length} hidden Session${hidden.length === 1 ? "" : "s"}?`}
        consequence="Their saved output, Commands, and Events are erased from their Machines. This cannot be undone."
        confirmLabel="Delete all"
        onConfirm={() => {
          setConfirming(false);
          void removal.remove(hidden.map((session) => session.id));
        }}
        onCancel={() => setConfirming(false)}
      />
    </div>
  );
}
