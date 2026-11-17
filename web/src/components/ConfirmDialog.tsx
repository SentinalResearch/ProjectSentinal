import { useEffect, useRef, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";

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
  /** Start on Cancel instead of the confirm button: for actions that cannot be undone. */
  focusCancel?: boolean;
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])';

/**
 * The destructive confirm dialog (Ctrl-C, revoke, etc.) — centered modal on desktop,
 * per SentinalSession.dc.html's actual confirmOpen markup (Foundations v2 §4.12 shows
 * a phone-width bottom sheet variant instead; that's the responsive/phone treatment).
 * Rule: destructive actions never complete on one accidental click — Esc or a scrim
 * click always cancels, never confirms. Focus stays inside while it is open and
 * returns to whatever opened it.
 */
export function ConfirmDialog({
  open,
  overline,
  question,
  consequence,
  confirmLabel,
  onConfirm,
  onCancel,
  focusCancel = false,
}: ConfirmDialogProps) {
  const panel = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onCancel]);

  useEffect(() => {
    if (!open) return;
    const opener = document.activeElement;
    const buttons = panel.current?.querySelectorAll<HTMLElement>("button") ?? [];
    (focusCancel ? buttons[0] : buttons[buttons.length - 1])?.focus();
    return () => {
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [open, focusCancel]);

  if (!open) return null;

  function trapTab(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.key !== "Tab" || !panel.current) return;
    const controls = Array.from(panel.current.querySelectorAll<HTMLElement>(FOCUSABLE));
    if (controls.length === 0) return;
    const first = controls[0];
    const last = controls[controls.length - 1];
    const inside = panel.current.contains(document.activeElement);
    if (event.shiftKey && (document.activeElement === first || !inside)) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (document.activeElement === last || !inside)) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div className={styles.scrim} onClick={onCancel}>
      <div
        ref={panel}
        className={styles.panel}
        role="dialog"
        aria-modal="true"
        aria-label={question}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={trapTab}
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
          <Button variant="destructive" onClick={onConfirm}>
            {confirmLabel}
          </Button>
        </div>
      </div>
    </div>
  );
}
