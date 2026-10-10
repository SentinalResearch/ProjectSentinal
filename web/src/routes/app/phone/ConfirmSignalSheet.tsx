import { BottomSheet } from "../../../components/BottomSheet";
import { formatSessionDuration } from "../../../data/format";
import type { Session } from "../../../data/types";
import styles from "./phoneSession.module.css";

export interface ConfirmSignalSheetProps {
  open: boolean;
  session: Session;
  machineName: string;
  onConfirm: () => void;
  onClose: () => void;
}

/** C2 · Ctrl-C confirmation: explains what will be interrupted; Cancel is the default. */
export function ConfirmSignalSheet({ open, session, machineName, onConfirm, onClose }: ConfirmSignalSheetProps) {
  const progress = session.progress ? ` and is at ${session.progress.unit} ${session.progress.current}/${session.progress.total}` : "";
  return (
    <BottomSheet open={open} title={`Send Ctrl-C to ${session.name}?`} onClose={onClose}>
      <p className={styles.sheetText}>
        This interrupts <code className="mono">{session.command ?? session.name}</code> on {machineName}. It has been running for{" "}
        {formatSessionDuration(session)}
        {progress}.
      </p>
      <button type="button" className={styles.destructiveButton} onClick={onConfirm}>
        Send Ctrl-C
      </button>
      <button type="button" className={styles.outlineButton} onClick={onClose}>
        Cancel
      </button>
    </BottomSheet>
  );
}
