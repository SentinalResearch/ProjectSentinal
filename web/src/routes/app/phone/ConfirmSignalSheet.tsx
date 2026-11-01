import { BottomSheet } from "../../../components/BottomSheet";
import { HoldButton } from "../../../components/HoldButton";
import { formatSessionDuration, lastCheckpointPath } from "../../../data/format";
import type { Session } from "../../../data/types";
import { CONFIRM_AFTER_SECONDS } from "./interrupt";
import styles from "./phoneSession.module.css";

export interface ConfirmSignalSheetProps {
  open: boolean;
  session: Session;
  machineName: string;
  onConfirm: () => void;
  onClose: () => void;
}

/**
 * M6 · ^C on a long job asks first (MOBILE_GLASS_SPEC §5): what it
 * interrupts, how long it has run, the last checkpoint, and a hold-to-send
 * button. Cancel is the default; jobs under 30 s never see this.
 */
export function ConfirmSignalSheet({ open, session, machineName, onConfirm, onClose }: ConfirmSignalSheetProps) {
  const progress = session.progress ? `, ${session.progress.unit} ${session.progress.current}/${session.progress.total}` : "";
  const checkpoint = lastCheckpointPath(session);
  const checkpointName = checkpoint?.split("/").pop()?.replace(/\.[^.]+$/, "");
  return (
    <BottomSheet open={open} title={`Send ^C to ${session.name}?`} onClose={onClose}>
      <p className={styles.sheetText}>
        Interrupts <code className="mono">{session.command ?? session.name}</code> on {machineName}. Running{" "}
        {formatSessionDuration(session)}
        {progress}.{checkpointName ? ` Last checkpoint: ${checkpointName}.` : ""}
      </p>
      <HoldButton onHeld={onConfirm}>Hold to send ^C</HoldButton>
      <button type="button" className={styles.outlineButton} onClick={onClose}>
        Cancel
      </button>
      <p className={styles.sheetNote}>Jobs under {CONFIRM_AFTER_SECONDS}s get ^C immediately.</p>
    </BottomSheet>
  );
}
