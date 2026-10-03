import { useCallback, useEffect, useRef, useState } from "react";

import { CopyButton } from "../../components/CopyButton";
import { dataAdapter } from "../../data";
import { RegistryError } from "../../data/adapter";
import type { PairingCode } from "../../protocol/PairingCode";
import type { PairingCodeStatus } from "../../protocol/PairingCodeStatus";
import styles from "./AddMachineDialog.module.css";

export interface AddMachineDialogProps {
  onClose: () => void;
  /** Called once a worker connects, so the registry can reload. */
  onConnected: () => void;
  /** Status polling interval (tests shorten it). */
  pollMs?: number;
}

type Phase =
  | { kind: "creating" }
  | { kind: "waiting"; code: PairingCode }
  | { kind: "connected"; machineId: string }
  | { kind: "expired" }
  | { kind: "error"; message: string };

function formatClock(ms: number): string {
  const totalSeconds = Math.max(0, Math.ceil(ms / 1000));
  return `${String(Math.floor(totalSeconds / 60)).padStart(2, "0")}:${String(totalSeconds % 60).padStart(2, "0")}`;
}

/**
 * Settings → Machines → Add Machine. The one-time code lives only in this
 * component's state: it is never put in a URL, storage, or the copied
 * command, and it is dropped as soon as the code is used, expires, or the
 * dialog closes (closing cancels an unused code).
 */
export function AddMachineDialog({ onClose, onConnected, pollMs = 2000 }: AddMachineDialogProps) {
  const [phase, setPhase] = useState<Phase>({ kind: "creating" });
  const [now, setNow] = useState(() => Date.now());
  const outstanding = useRef<string | null>(null);

  const create = useCallback(async () => {
    setPhase({ kind: "creating" });
    try {
      const code = await dataAdapter.registry.createCode();
      outstanding.current = code.code_id;
      setPhase({ kind: "waiting", code });
    } catch (error) {
      setPhase({ kind: "error", message: error instanceof RegistryError ? error.message : "Could not create a code." });
    }
  }, []);

  useEffect(() => {
    void create();
  }, [create]);

  // Poll the code's state; the code itself is never returned again.
  useEffect(() => {
    if (phase.kind !== "waiting") return;
    const codeId = phase.code.code_id;
    const timer = setInterval(() => {
      setNow(Date.now());
      void dataAdapter.registry.codeStatus(codeId).then(
        (status: PairingCodeStatus) => {
          if (status.state === "CONNECTED") {
            outstanding.current = null;
            setPhase({ kind: "connected", machineId: status.machine_id ?? "" });
            onConnected();
          } else if (status.state === "EXPIRED" || status.state === "CANCELLED") {
            outstanding.current = null;
            setPhase({ kind: "expired" });
          }
        },
        () => undefined,
      );
    }, pollMs);
    return () => clearInterval(timer);
  }, [phase, pollMs, onConnected]);

  const close = useCallback(() => {
    const codeId = outstanding.current;
    outstanding.current = null;
    if (codeId) void dataAdapter.registry.cancelCode(codeId).catch(() => undefined);
    onClose();
  }, [onClose]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") close();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [close]);

  // Unmounting (navigation) cancels too, so no usable code is left behind.
  useEffect(
    () => () => {
      const codeId = outstanding.current;
      if (codeId) void dataAdapter.registry.cancelCode(codeId).catch(() => undefined);
    },
    [],
  );

  return (
    <div className={styles.scrim}>
      <section className={styles.panel} role="dialog" aria-modal="true" aria-label="Add Machine">
        <div className={styles.head}>
          <span className={styles.overline}>ADD MACHINE</span>
          <button type="button" className={styles.closeBtn} aria-label="Close" onClick={close}>
            ×
          </button>
        </div>

        {phase.kind === "creating" && <p className={styles.description}>Creating a one-time code…</p>}

        {phase.kind === "error" && (
          <div className={styles.body}>
            <p className={styles.error} role="alert">
              {phase.message}
            </p>
            <div className={styles.actions}>
              <button type="button" className={styles.submit} onClick={() => void create()}>
                Try again
              </button>
              <button type="button" className={styles.copyBtn} onClick={close}>
                Cancel
              </button>
            </div>
          </div>
        )}

        {phase.kind === "waiting" && (
          <div className={styles.body}>
            <p className={styles.description}>
              On the already-installed worker, run this command, then type the code at the hidden prompt. The code
              works once.
            </p>
            <code className={`${styles.commandBlock} mono`}>{phase.code.connect_command}</code>
            <div className={styles.actions}>
              <CopyButton text={phase.code.connect_command} label="Copy command" />
            </div>
            <span className={styles.fieldLabel}>PAIRING CODE</span>
            <div className={styles.actions}>
              <span className={`${styles.code} mono`}>{phase.code.code}</span>
              {/* The code goes only to the clipboard; it is never stored. */}
              <CopyButton text={phase.code.code} label="Copy code" />
            </div>
            <div className={styles.ttlRow}>
              <span className={styles.ttlValue}>Expires in {formatClock(phase.code.expires_at - now)}</span>
            </div>
            <div className={styles.waitingRow}>Waiting for the worker to connect…</div>
            <div className={styles.actions}>
              <button type="button" className={styles.copyBtn} onClick={close}>
                Cancel
              </button>
            </div>
          </div>
        )}

        {phase.kind === "connected" && (
          <div className={styles.body}>
            <p className={styles.question}>Connected</p>
            <span className={`${styles.machineId} mono`}>{phase.machineId}</span>
            <p className={styles.description}>Rename it in the Machines list. The ID never changes.</p>
            <button type="button" className={styles.doneBtn} onClick={onClose}>
              Done
            </button>
          </div>
        )}

        {phase.kind === "expired" && (
          <div className={styles.body}>
            <p className={styles.description}>This code expired or was cancelled before a worker used it.</p>
            <div className={styles.actions}>
              <button type="button" className={styles.submit} onClick={() => void create()}>
                Create a new code
              </button>
              <button type="button" className={styles.copyBtn} onClick={onClose}>
                Close
              </button>
            </div>
          </div>
        )}
      </section>
    </div>
  );
}
