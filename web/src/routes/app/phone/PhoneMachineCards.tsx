import { useState } from "react";

import { BottomSheet } from "../../../components/BottomSheet";
import { dataAdapter } from "../../../data";
import { useLiveData } from "../../../data/LiveDataContext";
import type { RegistryMachine } from "../../../protocol/RegistryMachine";
import { MachinePanel } from "../MachinePanel";
import styles from "./phoneSettings.module.css";

const STATUS: Record<RegistryMachine["status"], string> = {
  ONLINE: "Online",
  OFFLINE: "Offline",
  DISCONNECTED: "Disconnected",
  REVOKED: "Revoked",
};

/** E2 · Machine cards on phones; Revoke always goes through a confirmation sheet. */
export function PhoneMachineCards({ machines, onChanged }: { machines: RegistryMachine[]; onChanged: () => Promise<void> }) {
  const [revoking, setRevoking] = useState<RegistryMachine | null>(null);
  const [details, setDetails] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const live = useLiveData();

  async function confirmRevoke() {
    if (!revoking) return;
    const target = revoking;
    setRevoking(null);
    setError(null);
    try {
      await dataAdapter.registry.revoke(target.machine_id);
      await onChanged();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "The control refused the change.");
    }
  }

  return (
    <div className={styles.cards}>
      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
      {machines.map((machine) => {
        const liveMachine = live.machines.find((candidate) => candidate.id === machine.machine_id);
        const expanded = details === machine.machine_id;
        return (
          <article key={machine.machine_id} className={styles.card} aria-label={machine.display_name}>
            <div className={styles.cardTop}>
              <strong className={styles.cardName}>{machine.display_name}</strong>
              <span className={styles.roleTag}>{machine.role}</span>
            </div>
            <span className={styles.cardMeta}>{liveMachine?.hardware ?? (machine.os === "LINUX" ? "Linux" : machine.os === "MACOS" ? "macOS" : "—")}</span>
            <span className={styles.cardStatus} data-online={machine.status === "ONLINE"}>
              {STATUS[machine.status]}
            </span>
            <div className={styles.cardActions}>
              {liveMachine && (
                <button type="button" className={styles.textButton} aria-expanded={expanded} onClick={() => setDetails(expanded ? null : machine.machine_id)}>
                  {expanded ? "Hide details" : "Load and processes"}
                </button>
              )}
              {machine.role === "WORKER" && machine.status !== "REVOKED" && (
                <button type="button" className={styles.destructiveText} onClick={() => setRevoking(machine)}>
                  Revoke…
                </button>
              )}
            </div>
            {expanded && liveMachine && (
              <MachinePanel machine={liveMachine} sessions={live.terminals.filter((session) => session.machine === liveMachine.id)} />
            )}
          </article>
        );
      })}
      <BottomSheet open={revoking !== null} title={`Revoke ${revoking?.display_name ?? ""}?`} onClose={() => setRevoking(null)}>
        <p className={styles.sheetText}>
          Its token stops working immediately. Other Machines are not affected. To use it again, pair it with a new code.
        </p>
        <button type="button" className={styles.destructiveButton} onClick={() => void confirmRevoke()}>
          Revoke
        </button>
        <button type="button" className={styles.outlineButton} onClick={() => setRevoking(null)}>
          Cancel
        </button>
      </BottomSheet>
    </div>
  );
}
