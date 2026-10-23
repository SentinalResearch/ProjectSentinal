import { useCallback, useEffect, useState } from "react";

import { BottomSheet } from "../../components/BottomSheet";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { WorkerUpdateError, workerUpdatesApi } from "../../data/api/workerUpdates";
import { useIsPhone } from "../../hooks/useViewport";
import type { RegistryMachine } from "../../protocol/RegistryMachine";
import styles from "./ActionCard.module.css";

interface Pending {
  machine: RegistryMachine;
  /** Set when the worker answered LIVE_SESSIONS. */
  interrupt: string | null;
}

/** "This ends 2 live terminals on RTX PC." from the worker's LIVE_SESSIONS refusal. */
function endsText(refusal: string, machine: string): string {
  const count = /^(\d+) live/.exec(refusal)?.[1];
  if (count === undefined) return refusal.replace(/; confirm.*$/, ".");
  return `This ends ${count} live terminal${count === "1" ? "" : "s"} on ${machine}.`;
}

function statusText(machine: RegistryMachine): string | null {
  const update = machine.update;
  if (!update) return null;
  switch (update.state) {
    case "STARTED":
      return `Updating to ${update.version}…`;
    case "INSTALLED":
      return `Updated to ${update.version}.`;
    case "FAILED":
      return `Update to ${update.version} failed: ${update.reason ?? "see the worker's worker-update.log"}.`;
    case "UNKNOWN":
      return `No result yet for the update to ${update.version}; check the Machine with sentinal doctor.`;
  }
}

/**
 * Workers behind the release on this control's stable channel (MILESTONE 4.E
 * §11). Each updates on its own, from its own pinned control, after a
 * confirmation; live terminals need a second, explicit one.
 */
export function WorkerUpdatesCard({ pollMs = 5_000 }: { pollMs?: number }) {
  const [available, setAvailable] = useState<string | null>(null);
  const [workers, setWorkers] = useState<RegistryMachine[]>([]);
  const [pending, setPending] = useState<Pending | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const isPhone = useIsPhone();

  const refresh = useCallback(async () => {
    try {
      const registry = await workerUpdatesApi.registry();
      setAvailable(registry.available_release ?? null);
      setWorkers(registry.machines.filter((machine) => machine.role === "WORKER" && machine.status !== "REVOKED"));
    } catch {
      setAvailable(null);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), pollMs);
    return () => window.clearInterval(timer);
  }, [refresh, pollMs]);

  if (!available) return null;
  const behind = workers.filter((machine) => machine.daemon_version !== available || machine.update?.state === "STARTED");
  if (behind.length === 0) return null;

  async function start(target: Pending) {
    setBusy(true);
    setError(null);
    try {
      await workerUpdatesApi.start(target.machine.machine_id, available!, target.interrupt !== null);
      setPending(null);
      await refresh();
    } catch (failure) {
      if (failure instanceof WorkerUpdateError && failure.code === "LIVE_SESSIONS" && target.interrupt === null) {
        setPending({ machine: target.machine, interrupt: failure.message });
      } else {
        setPending(null);
        setError(failure instanceof Error ? failure.message : "The update could not start.");
      }
    } finally {
      setBusy(false);
    }
  }

  const question = pending
    ? `Update ${pending.machine.display_name} from ${pending.machine.daemon_version ?? "its release"} to ${available}?`
    : "";
  const consequence = pending?.interrupt
    ? `${endsText(pending.interrupt, pending.machine.display_name)} The Machine restarts sentinald and reconnects on its own.`
    : "The Machine installs the release from this control, verifies it, restarts sentinald, and keeps its previous release if anything fails.";
  const confirmLabel = pending?.interrupt ? "End terminals and update" : `Update to ${available}`;

  return (
    <section className={styles.card} aria-label="Worker updates">
      <span className={styles.overline}>WORKER UPDATES</span>
      <p className={styles.muted}>
        {available} on stable · {behind.length} of {workers.length} workers behind
      </p>
      {behind.map((machine) => {
        const status = statusText(machine);
        const running = machine.update?.state === "STARTED";
        return (
          <div key={machine.machine_id} role="group" aria-label={machine.display_name} className={styles.request}>
            <div className={styles.who}>
              <strong>{machine.display_name}</strong>
              <span className={styles.identity}>
                {machine.daemon_version ?? "unknown"} → {available}
                {machine.status !== "ONLINE" && " · offline"}
              </span>
              {status && (
                <span className={machine.update?.state === "FAILED" ? styles.problem : styles.status} role="status">
                  {status}
                </span>
              )}
            </div>
            {!running && (
              <div className={styles.actions}>
                <button
                  type="button"
                  className={styles.primary}
                  disabled={busy || machine.status !== "ONLINE"}
                  onClick={() => setPending({ machine, interrupt: null })}
                >
                  Update
                </button>
              </div>
            )}
          </div>
        );
      })}
      {error && (
        <p className={styles.problem} role="alert">
          {error}
        </p>
      )}
      {pending &&
        (isPhone ? (
          <BottomSheet open title={question} onClose={() => setPending(null)}>
            <p className={styles.muted}>{consequence}</p>
            <button type="button" className={styles.primary} disabled={busy} onClick={() => void start(pending)}>
              {confirmLabel}
            </button>
            <button type="button" className={styles.secondary} onClick={() => setPending(null)}>
              Cancel
            </button>
          </BottomSheet>
        ) : (
          <ConfirmDialog
            open
            overline="UPDATE WORKER"
            question={question}
            consequence={consequence}
            confirmLabel={confirmLabel}
            onConfirm={() => void start(pending)}
            onCancel={() => setPending(null)}
          />
        ))}
    </section>
  );
}
