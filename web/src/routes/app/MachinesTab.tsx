import { useCallback, useEffect, useState, type FormEvent } from "react";

import { dataAdapter } from "../../data";
import { RegistryError } from "../../data/adapter";
import { useLiveData } from "../../data/LiveDataContext";
import type { RegistryMachine } from "../../protocol/RegistryMachine";
import { AddMachineDialog } from "./AddMachineDialog";
import styles from "./SettingsPage.module.css";

export interface MachinesTabProps {
  /** Pairing-code polling interval (tests shorten it). */
  pollMs?: number;
}

type Load =
  | { kind: "loading" }
  | { kind: "ready"; machines: RegistryMachine[] }
  | { kind: "notControl"; message: string }
  | { kind: "error"; message: string };

/**
 * The control Machine's registry. Rows are keyed and acted on by the
 * immutable Machine ID; the display name is only a label. Revoke uses the
 * inline Revoke?/Revoke/Keep confirm from the design, never one click.
 */
export function MachinesTab({ pollMs }: MachinesTabProps) {
  const [load, setLoad] = useState<Load>({ kind: "loading" });
  const [adding, setAdding] = useState(false);

  const reload = useCallback(async () => {
    try {
      setLoad({ kind: "ready", machines: await dataAdapter.registry.list() });
    } catch (error) {
      if (error instanceof RegistryError && error.code === "NOT_CONTROL") {
        setLoad({ kind: "notControl", message: error.message });
      } else {
        setLoad({ kind: "error", message: error instanceof Error ? error.message : "Could not load Machines." });
      }
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  if (load.kind === "loading") return <div className={styles.tabLoading}>Loading…</div>;
  if (load.kind === "notControl") return <NotControl />;

  return (
    <>
      <header className={styles.tabHeader}>
        <div className={styles.tabHeaderText}>
          <h2 className={styles.tabTitle}>Machines</h2>
          <span className={styles.tabSubtitle}>
            Machines enrolled with this control. Each has a permanent ID and its own revocable credential.
          </span>
        </div>
        <button type="button" className={styles.primaryBtn} onClick={() => setAdding(true)}>
          Add Machine
        </button>
      </header>

      {load.kind === "error" ? (
        <p className={styles.tabLoading} role="alert">
          {load.message}
        </p>
      ) : (
        <RegistryTable machines={load.machines} onChanged={reload} />
      )}

      {adding && (
        <AddMachineDialog onClose={() => setAdding(false)} onConnected={() => void reload()} pollMs={pollMs} />
      )}
    </>
  );
}

function RegistryTable({ machines, onChanged }: { machines: RegistryMachine[]; onChanged: () => Promise<void> }) {
  const counts = new Map<string, number>();
  for (const machine of machines) counts.set(machine.display_name, (counts.get(machine.display_name) ?? 0) + 1);

  return (
    <div className={styles.tableScroll} role="table" aria-label="Machines">
      <div className={styles.machineHeadRow} role="presentation">
        <span>MACHINE</span>
        <span>ROLE</span>
        <span>STATUS</span>
        <span>ADDRESS</span>
        <span />
      </div>
      {machines.map((machine) => (
        <RegistryRow
          key={machine.machine_id}
          machine={machine}
          duplicateName={(counts.get(machine.display_name) ?? 0) > 1}
          onChanged={onChanged}
        />
      ))}
      {machines.length === 0 && <div className={styles.tabLoading}>No Machines yet.</div>}
    </div>
  );
}

function RegistryRow({
  machine,
  duplicateName,
  onChanged,
}: {
  machine: RegistryMachine;
  duplicateName: boolean;
  onChanged: () => Promise<void>;
}) {
  const [renaming, setRenaming] = useState(false);
  const [name, setName] = useState(machine.display_name);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const online = machine.status === "ONLINE";
  const canRevoke = machine.role === "WORKER" && machine.status !== "REVOKED";

  async function act(action: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      setRenaming(false);
      setConfirming(false);
      await onChanged();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "The control refused the change.");
    } finally {
      setBusy(false);
    }
  }

  function saveName(event: FormEvent) {
    event.preventDefault();
    void act(() => dataAdapter.registry.rename(machine.machine_id, name));
  }

  return (
    <div className={styles.machineRow} role="row">
      <div className={styles.machineNameCol}>
        {renaming ? (
          <form onSubmit={saveName}>
            <input
              aria-label="Display name"
              className={styles.renameInput}
              value={name}
              maxLength={64}
              onChange={(event) => setName(event.target.value)}
            />
            <button type="submit" className={styles.keepBtn} disabled={busy}>
              Save
            </button>
            <button type="button" className={styles.keepBtn} onClick={() => setRenaming(false)}>
              Cancel
            </button>
          </form>
        ) : (
          <span className={styles.machineName}>{machine.display_name}</span>
        )}
        <span className={styles.machineOs}>
          {machine.os === "LINUX" ? "Linux" : machine.os === "MACOS" ? "macOS" : "—"}
          {duplicateName && (
            <>
              {" · "}
              <span className="mono" title={machine.machine_id}>
                {machine.machine_id.slice(0, 6)}
              </span>
            </>
          )}
          {machine.capabilities.length > 0 && ` · ${machine.capabilities.join(", ")}`}
          {machine.credential && ` · credential ${machine.credential.expires_at <= Date.now() ? "expired" : "active"}`}
        </span>
        {error && (
          <span className={styles.rowError} role="alert">
            {error}
          </span>
        )}
      </div>
      <span className={styles.machineHw}>{machine.role}</span>
      <span className={styles.machineStatus} style={{ color: online ? "var(--text-1)" : "var(--text-3)" }}>
        <span
          className={styles.statusDot}
          style={{
            background: online ? "var(--s-running)" : "transparent",
            boxShadow: online ? "none" : "inset 0 0 0 1.5px var(--s-disconnected-fg)",
          }}
        />
        {statusLabel(machine)}
      </span>
      <span className={`${styles.machineToken} mono`}>
        {machine.endpoint ? `${machine.endpoint.address}:${machine.endpoint.port}` : "—"}
      </span>
      <div className={styles.machineActions}>
        {confirming ? (
          <>
            <span className={styles.revokeConfirmLabel}>Revoke {machine.display_name}?</span>
            <button
              type="button"
              className={styles.revokeBtn}
              disabled={busy}
              onClick={() => void act(() => dataAdapter.registry.revoke(machine.machine_id))}
            >
              Revoke
            </button>
            <button type="button" className={styles.keepBtn} onClick={() => setConfirming(false)}>
              Keep
            </button>
          </>
        ) : (
          <>
            {!renaming && machine.status !== "REVOKED" && (
              <button
                type="button"
                className={styles.rotateBtn}
                onClick={() => {
                  setName(machine.display_name);
                  setRenaming(true);
                }}
              >
                Rename
              </button>
            )}
            {!online && machine.status !== "REVOKED" && (
              <button type="button" className={styles.rotateBtn} disabled={busy} onClick={() => void onChanged()}>
                Reconnect
              </button>
            )}
            {canRevoke && (
              <button type="button" className={styles.revokeEntry} onClick={() => setConfirming(true)}>
                Revoke…
              </button>
            )}
          </>
        )}
      </div>
    </div>
  );
}

function statusLabel(machine: RegistryMachine): string {
  if (machine.credential && machine.credential.expires_at <= Date.now()) {
    return machine.last_seen
      ? `Authentication expired · last seen ${new Date(machine.last_seen).toLocaleString()}`
      : "Authentication expired";
  }
  switch (machine.status) {
    case "ONLINE":
      return "Online";
    case "OFFLINE":
      return machine.last_seen ? `Offline · last seen ${new Date(machine.last_seen).toLocaleString()}` : "Offline · never seen";
    case "DISCONNECTED":
      return "Disconnected";
    case "REVOKED":
      return "Revoked";
  }
}

function NotControl() {
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
        <strong>This daemon is not a control Machine.</strong>
        <span>
          Enable <code>[control]</code> in the daemon.toml of the Machine you choose as control to add other Machines.
        </span>
      </div>
      <div className={styles.connectedMachineList}>
        {machines.map((machine) => (
          <div key={machine.id} className={styles.connectedMachine}>
            <div className={styles.machineNameCol}>
              <span className={styles.machineName}>{machine.name}</span>
              <span className={styles.machineOs}>{machine.os}</span>
            </div>
            <span className={styles.machineHw}>{machine.hardware}</span>
          </div>
        ))}
      </div>
    </>
  );
}
