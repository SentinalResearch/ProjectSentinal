import { useEffect, useState } from "react";

import { dataAdapter, isMockData } from "../../data";
import { useLiveData } from "../../data/LiveDataContext";
import type { MachineToken } from "../../data/types";
import styles from "./SettingsPage.module.css";

export interface MachinesTabProps {
  onOpenPair: () => void;
}

/**
 * Revoke uses an inline row-swap confirm ("Revoke?" + Revoke/Keep in place of
 * Rotate/Revoke…), not the modal `ConfirmDialog` — this is what
 * SentinalSettings.dc.html's own working prototype actually does (`m.confirming`
 * state swaps the action cell's buttons in place), distinct from Session
 * Detail's Ctrl-C modal. Still satisfies rule 5: revoke never completes on one click.
 */
export function MachinesTab({ onOpenPair }: MachinesTabProps) {
  return isMockData ? <MachineTokens onOpenPair={onOpenPair} /> : <ConnectedMachine />;
}

function ConnectedMachine() {
  const { machines } = useLiveData();

  return (
    <>
      <header className={styles.tabHeader}>
        <div className={styles.tabHeaderText}>
          <h2 className={styles.tabTitle}>Machines</h2>
          <span className={styles.tabSubtitle}>The Machine served by the daemon configured for this web app.</span>
        </div>
      </header>

      <div className={styles.unavailablePanel}>
        <strong>Pairing and token management are not available yet.</strong>
        <span>This build connects to one daemon. Multi-Machine enrollment and aggregation are a later milestone.</span>
      </div>

      <div className={styles.connectedMachineList}>
        {machines.map((machine) => (
          <div key={machine.id} className={styles.connectedMachine}>
            <div className={styles.machineNameCol}>
              <span className={styles.machineName}>{machine.name}</span>
              <span className={styles.machineOs}>{machine.os}</span>
            </div>
            <span className={styles.machineHw}>{machine.hardware}</span>
            <span className={styles.machineStatus}>
              <span
                className={styles.statusDot}
                style={{
                  background: machine.status === "ONLINE" ? "var(--s-running)" : "transparent",
                  boxShadow: machine.status === "ONLINE" ? "none" : "inset 0 0 0 1.5px var(--s-disconnected-fg)",
                }}
              />
              {machine.status === "ONLINE" ? "Online" : "Offline"}
            </span>
          </div>
        ))}
        {machines.length === 0 && <span className={styles.tabLoading}>No Machine is available from the daemon.</span>}
      </div>
    </>
  );
}

function MachineTokens({ onOpenPair }: MachinesTabProps) {
  const { machines } = useLiveData();
  const [tokens, setTokens] = useState<MachineToken[] | null>(null);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

  useEffect(() => {
    dataAdapter.getMachineTokens().then(setTokens);
    // Re-fetch when the machine list changes (e.g. a newly paired machine appears)
    // so its token row shows up without a manual refresh.
  }, [machines.length]);

  async function rotate(machineId: string) {
    setBusyId(machineId);
    const updated = await dataAdapter.rotateToken(machineId);
    setTokens((prev) => (prev ?? []).map((t) => (t.machineId === machineId ? updated : t)));
    setBusyId(null);
  }

  async function revoke(machineId: string) {
    setBusyId(machineId);
    await dataAdapter.revokeToken(machineId);
    setTokens((prev) => (prev ?? []).filter((t) => t.machineId !== machineId));
    setConfirmingId(null);
    setBusyId(null);
  }

  if (!tokens) return <div className={styles.tabLoading}>Loading…</div>;

  return (
    <>
      <header className={styles.tabHeader}>
        <div className={styles.tabHeaderText}>
          <h2 className={styles.tabTitle}>Machines &amp; tokens</h2>
          <span className={styles.tabSubtitle}>Each machine runs an agent that authenticates with its own token.</span>
        </div>
        <button type="button" className={styles.primaryBtn} onClick={onOpenPair}>
          Pair new machine
        </button>
      </header>

      <div className={styles.tableScroll}>
        <div className={styles.machineHeadRow}>
          <span>MACHINE</span>
          <span>HARDWARE</span>
          <span>STATUS</span>
          <span>TOKEN</span>
          <span />
        </div>
        {tokens.map((t) => {
          const machine = machines.find((m) => m.id === t.machineId);
          const online = machine?.status === "ONLINE";
          const confirming = confirmingId === t.machineId;
          return (
            <div key={t.machineId} className={styles.machineRow}>
              <div className={styles.machineNameCol}>
                <span className={styles.machineName}>{t.machineName}</span>
                <span className={styles.machineOs}>{machine?.os ?? "—"}</span>
              </div>
              <span className={styles.machineHw}>{machine?.hardware ?? "—"}</span>
              <span className={styles.machineStatus} style={{ color: online ? "var(--text-1)" : "var(--text-3)" }}>
                <span
                  className={styles.statusDot}
                  style={{
                    background: online ? "var(--s-running)" : "transparent",
                    boxShadow: online ? "none" : "inset 0 0 0 1.5px var(--s-disconnected-fg)",
                  }}
                />
                {online ? "Online" : `Offline · ${machine?.lastSeen ?? "—"} ago`}
              </span>
              <span className={`${styles.machineToken} mono`}>{t.maskedToken}</span>
              <div className={styles.machineActions}>
                {confirming ? (
                  <>
                    <span className={styles.revokeConfirmLabel}>Revoke?</span>
                    <button
                      type="button"
                      className={styles.revokeBtn}
                      disabled={busyId === t.machineId}
                      onClick={() => revoke(t.machineId)}
                    >
                      Revoke
                    </button>
                    <button type="button" className={styles.keepBtn} onClick={() => setConfirmingId(null)}>
                      Keep
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      className={styles.rotateBtn}
                      disabled={busyId === t.machineId}
                      onClick={() => rotate(t.machineId)}
                    >
                      Rotate
                    </button>
                    <button type="button" className={styles.revokeEntry} onClick={() => setConfirmingId(t.machineId)}>
                      Revoke…
                    </button>
                  </>
                )}
              </div>
            </div>
          );
        })}
        {tokens.length === 0 && <div className={styles.tabLoading}>No machines paired yet.</div>}
      </div>
    </>
  );
}
