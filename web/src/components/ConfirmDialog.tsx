import { useEffect, type ReactNode } from "react";

import { Button } from "./Button";
import styles from "./ConfirmDialog.module.css";

export interface ConfirmDialogProps {
  open: boolean;
  overline: string;
  question: string;
  consequence: ReactNode;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}

/**
 * The destructive confirm dialog (Ctrl-C, revoke, etc.) — centered modal on desktop,
 * per SentinalSession.dc.html's actual confirmOpen markup (Foundations v2 §4.12 shows
 * a phone-width bottom sheet variant instead; that's the responsive/phone treatment).
 * Rule: destructive actions never complete on one accidental click — Esc or a scrim
 * click always cancels, never confirms.
 */
export function ConfirmDialog({
  open,
  overline,
  question,
  consequence,
  confirmLabel,
  onConfirm,
  onCancel,
}: ConfirmDialogProps) {
  useEffect(() => {
    if (!open) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onCancel]);

  if (!open) return null;

  return (
    <div className={styles.scrim} onClick={onCancel}>
      <div
        className={styles.panel}
        role="dialog"
        aria-modal="true"
        aria-label={question}
        onClick={(e) => e.stopPropagation()}
      >
        <div className={styles.head}>
          <span className={styles.overline}>{overline}</span>
          <span className={styles.question}>{question}</span>
          <span className={styles.consequence}>{consequence}</span>
        </div>
        <div className={styles.actions}>
          <Button variant="secondary" onClick={onCancel}>
            Cancel
          </Button>
          <Button variant="destructive" onClick={onConfirm} autoFocus>
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}
