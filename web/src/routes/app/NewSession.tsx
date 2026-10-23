import { useState, type ReactElement } from "react";
import { useNavigate } from "react-router-dom";

import { BottomSheet } from "../../components/BottomSheet";
import { dataAdapter } from "../../data";
import { useLiveData } from "../../data/LiveDataContext";
import styles from "./NewSession.module.css";

export interface NewSession {
  /** Start on `preferred` when it is online, the only online Machine, or else ask. */
  start: (preferred?: string | null) => void;
  /** The Machine chooser; render it once next to the button. */
  chooser: ReactElement;
}

/**
 * "New session" on any online Machine. Worker shells start through the
 * control's routed connection, so a cloud VM needs no SSH to open a terminal.
 */
export function useNewSession(pathFor: (sessionId: string) => string): NewSession {
  const navigate = useNavigate();
  const { machines } = useLiveData();
  const [choosing, setChoosing] = useState(false);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const online = machines.filter((machine) => machine.status === "ONLINE");

  async function startOn(machineId: string) {
    setStarting(true);
    setError(null);
    try {
      const session = await dataAdapter.createSession({ machineId });
      setChoosing(false);
      navigate(pathFor(session.id));
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : String(failure));
      setChoosing(true);
    } finally {
      setStarting(false);
    }
  }

  function start(preferred?: string | null) {
    const target = online.some((machine) => machine.id === preferred)
      ? preferred
      : online.length === 1
        ? online[0].id
        : null;
    if (target) void startOn(target);
    else {
      setError(null);
      setChoosing(true);
    }
  }

  const chooser = (
    <BottomSheet open={choosing} title="Start a Session on" onClose={() => setChoosing(false)}>
      {error && (
        <p className={styles.error} role="alert">
          {error}
        </p>
      )}
      {online.length === 0 && <p className={styles.text}>No Machine is online.</p>}
      {online.map((machine) => (
        <button
          key={machine.id}
          type="button"
          className={styles.machine}
          disabled={starting}
          onClick={() => void startOn(machine.id)}
        >
          <span className={styles.name}>{machine.name}</span>
          <span className={styles.detail}>{machine.os} · {machine.hardware}</span>
        </button>
      ))}
      <button type="button" className={styles.cancel} onClick={() => setChoosing(false)}>
        Cancel
      </button>
    </BottomSheet>
  );

  return { start, chooser };
}
