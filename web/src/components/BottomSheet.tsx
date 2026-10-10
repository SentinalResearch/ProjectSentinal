import { useEffect, useRef, type ReactNode, type KeyboardEvent as ReactKeyboardEvent } from "react";

import styles from "./BottomSheet.module.css";

export interface BottomSheetProps {
  open: boolean;
  title: string;
  onClose: () => void;
  children: ReactNode;
}

const FOCUSABLE = 'button:not([disabled]), [href], input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])';

/**
 * Phone bottom sheet (MOBILE_SPEC.md C2/D2). Escape, the scrim, and a downward
 * drag on the grabber only close it; anything destructive must be an explicit
 * button inside. Focus starts on Cancel (or the last control), stays inside,
 * and returns to the opener.
 */
export function BottomSheet({ open, title, onClose, children }: BottomSheetProps) {
  const panel = useRef<HTMLDivElement>(null);
  const opener = useRef<Element | null>(null);
  const dragStart = useRef<number | null>(null);

  useEffect(() => {
    if (!open) return;
    opener.current = document.activeElement;
    const controls = panel.current ? Array.from(panel.current.querySelectorAll<HTMLElement>(FOCUSABLE)) : [];
    const cancel = controls.find((control) => /^cancel$/i.test(control.textContent?.trim() ?? ""));
    (cancel ?? controls[controls.length - 1] ?? panel.current)?.focus();
    return () => {
      if (opener.current instanceof HTMLElement) opener.current.focus();
    };
  }, [open]);

  if (!open) return null;

  function onKeyDown(event: ReactKeyboardEvent<HTMLDivElement>) {
    if (event.key === "Escape") {
      event.stopPropagation();
      onClose();
      return;
    }
    if (event.key !== "Tab" || !panel.current) return;
    const controls = Array.from(panel.current.querySelectorAll<HTMLElement>(FOCUSABLE));
    if (controls.length === 0) return;
    const first = controls[0];
    const last = controls[controls.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  return (
    <div className={styles.layer}>
      <div className={styles.scrim} data-testid="sheet-scrim" onClick={onClose} aria-hidden="true" />
      <div
        ref={panel}
        className={styles.sheet}
        role="dialog"
        aria-modal="true"
        aria-label={title}
        tabIndex={-1}
        onKeyDown={onKeyDown}
      >
        <div
          className={styles.grabber}
          aria-hidden="true"
          onPointerDown={(event) => {
            dragStart.current = event.clientY;
          }}
          onPointerUp={(event) => {
            if (dragStart.current !== null && event.clientY - dragStart.current > 60) onClose();
            dragStart.current = null;
          }}
        />
        <h2 className={styles.title}>{title}</h2>
        {children}
      </div>
    </div>
  );
}
