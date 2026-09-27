import { useEffect, useState, type FormEvent } from "react";

import { dataAdapter } from "../../data";
import { useLiveData } from "../../data/LiveDataContext";
import type { PairingToken } from "../../data/types";
import styles from "./PairMachineDialog.module.css";

export interface PairMachineDialogProps {
  open: boolean;
  onClose: () => void;
}

function formatClock(ms: number): string {
  const totalSeconds = Math.max(0, Math.ceil(ms / 1000));
  const mm = String(Math.floor(totalSeconds / 60)).padStart(2, "0");
  const ss = String(totalSeconds % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

/**
 * design_handoff_sentinal/design/SentinalSettings.dc.html's pairing modal (3 steps).
 * The prototype has a manual "Prototype: simulate agent connected" button because
 * its own mock had no real timer — this build's mock adapter (`pairMachine`) really
 * does add the machine and emit MACHINE_UPDATED after ~5s, so step 3 is detected from
 * live data instead of a manual button.
 */
export function PairMachineDialog({ open, onClose }: PairMachineDialogProps) {
  const { machines } = useLiveData();
  const [step, setStep] = useState<1 | 2 | 3>(1);
  const [name, setName] = useState("");
  const [pairing, setPairing] = useState<PairingToken | null>(null);
  const [copied, setCopied] = useState(false);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (open) {
      setStep(1);
      setName("");
      setPairing(null);
      setCopied(false);
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  useEffect(() => {
    if (step !== 2) return;
    const interval = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(interval);
  }, [step]);

  useEffect(() => {
    if (step !== 2 || !pairing) return;
    if (machines.some((m) => m.name === pairing.machineName)) setStep(3);
  }, [step, pairing, machines]);

  if (!open) return null;

  async function submitName(e: FormEvent) {
    e.preventDefault();
    const trimmed = name.trim();
    if (!trimmed) return;
    const result = await dataAdapter.pairMachine(trimmed);
    setPairing(result);
    setStep(2);
  }

  async function copy() {
    if (!pairing) return;
    try {
      await navigator.clipboard.writeText(pairing.command);
    } catch {
      /* clipboard unavailable — Copy label just won't flip to Copied */
    }
    setCopied(true);
  }

  const remaining = pairing ? pairing.expiresAt - now : 0;

  return (
    <div className={styles.scrim} onClick={onClose}>
      <div className={styles.panel} role="dialog" aria-modal="true" onClick={(e) => e.stopPropagation()}>
        <div className={styles.head}>
          <span className={styles.overline}>PAIR NEW MACHINE · STEP {step} OF 3</span>
          <button type="button" className={styles.closeBtn} onClick={onClose} aria-label="Close">
            ×
          </button>
        </div>

        {step === 1 && (
          <form onSubmit={submitName} className={styles.body}>
            <span className={styles.question}>Name the machine</span>
            <label className={styles.field}>
              <span className={styles.fieldLabel}>Display name</span>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                autoFocus
                placeholder="e.g. Office PC"
                className={styles.input}
              />
            </label>
            <button
              type="submit"
              disabled={!name.trim()}
              className={styles.submit}
              style={{
                background: name.trim() ? "var(--accent)" : "var(--bg-2)",
                color: name.trim() ? "var(--on-accent)" : "var(--text-3)",
              }}
            >
              Generate pairing token
            </button>
          </form>
        )}

        {step === 2 && pairing && (
          <div className={styles.body}>
            <span className={styles.question}>Run this on {pairing.machineName}</span>
            <div className={styles.commandBlock}>{pairing.command}</div>
            <div className={styles.ttlRow}>
              <span>
                One-time token · expires in <b className={styles.ttlValue}>{formatClock(remaining)}</b>
              </span>
              <button type="button" className={styles.copyBtn} onClick={copy}>
                {copied ? "Copied" : "Copy command"}
              </button>
            </div>
            <div className={styles.waitingRow}>
              <span className={styles.fieldLabel}>Waiting for the agent to connect…</span>
              <div className={styles.progressTrack}>
                <div className={`${styles.progressBar} tg-motion`} />
              </div>
            </div>
          </div>
        )}

        {step === 3 && pairing && (
          <div className={styles.body}>
            <span className={styles.question}>{pairing.machineName} is online</span>
            <span className={styles.description}>
              It now appears in the sidebar and on the overview. The pairing token has been used and cannot be reused.
            </span>
            <button type="button" className={styles.doneBtn} onClick={onClose}>
              Done
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
