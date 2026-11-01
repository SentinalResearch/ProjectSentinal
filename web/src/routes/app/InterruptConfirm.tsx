import { ConfirmDialog } from "../../components/ConfirmDialog";
import { dataAdapter } from "../../data";
import { confirmConsequence } from "../../data/format";
import type { Session } from "../../data/types";

/**
 * The one Ctrl-C confirmation (Session page, prompt `kill`, dashboard Kill…):
 * nothing interrupts a program on a single click.
 */
export function InterruptConfirm({
  session,
  machineName,
  onClose,
}: {
  /** The Session to interrupt; the dialog is closed while undefined. */
  session: Session | undefined;
  machineName: string;
  onClose: () => void;
}) {
  return (
    <ConfirmDialog
      open={Boolean(session)}
      overline={`INTERRUPT · ${machineName.toUpperCase()}`}
      question={`Send Ctrl‑C to ${session?.name ?? ""}?`}
      consequence={session ? confirmConsequence(session, machineName) : ""}
      confirmLabel="Send Ctrl‑C"
      onConfirm={() => {
        if (session) void dataAdapter.sendSignal(session.id, "SIGINT");
        onClose();
      }}
      onCancel={onClose}
    />
  );
}
