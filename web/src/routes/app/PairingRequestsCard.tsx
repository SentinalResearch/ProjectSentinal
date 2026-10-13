import { useCallback, useEffect, useState } from "react";

import { pairingApi } from "../../data/api/pairing";
import type { PairingRequestView } from "../../protocol/PairingRequestView";
import styles from "./PairingRequestsCard.module.css";

function minutesLeft(expiresAt: number): string {
  const minutes = Math.max(0, Math.ceil((expiresAt - Date.now()) / 60_000));
  return minutes <= 1 ? "expires in under a minute" : `expires in ${minutes} min`;
}

/**
 * Machines asking this control to approve them (MILESTONE 4.E §6–7). Each
 * request shows the four words its own terminal shows; the owner approves the
 * one whose words match. There is deliberately no "approve all".
 */
export function PairingRequestsCard({ pollMs = 3_000 }: { pollMs?: number }) {
  const [requests, setRequests] = useState<PairingRequestView[]>([]);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const list = await pairingApi.list();
      setRequests(list.requests.filter((item) => item.verified && (item.state === "PENDING" || item.state === "APPROVED")));
    } catch {
      // Not a control, or signed out: show nothing.
      setRequests([]);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), pollMs);
    return () => window.clearInterval(timer);
  }, [refresh, pollMs]);

  async function decide(requestId: string, action: (id: string) => Promise<unknown>) {
    setBusy(requestId);
    setError(null);
    try {
      await action(requestId);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "The control refused the change.");
    } finally {
      setBusy(null);
      await refresh();
    }
  }

  if (requests.length === 0) return null;
  return (
    <section className={styles.card} aria-label="Machines waiting for approval">
      <span className={styles.overline}>WAITING FOR APPROVAL</span>
      <p className={styles.muted}>Approve a Machine only if its terminal shows the same four words.</p>
      {requests.map((item) => {
        const words = item.verification_words.join(" · ");
        const identity = item.tailscale;
        return (
          <div key={item.request_id} role="group" aria-label={`${item.claimed.display_name}: ${item.verification_words.join(" ")}`} className={styles.request}>
            <div className={styles.who}>
              <strong>{item.claimed.display_name}</strong>
              {identity && (
                <span className={styles.identity}>
                  {identity.node} · {identity.login} · {identity.os} · {identity.address}
                </span>
              )}
              {item.existing_machine && (
                <span className={styles.identity}>
                  Reconnects {item.existing_machine.display_name} ({item.existing_machine.status === "REVOKED" ? "revoked" : "disconnected"})
                </span>
              )}
            </div>
            <span className={styles.words}>{words}</span>
            {item.state === "APPROVED" ? (
              <p className={styles.status} role="status">
                Approved — waiting for the Machine to connect
              </p>
            ) : (
              <div className={styles.actions}>
                <span className={styles.muted}>{minutesLeft(item.expires_at)}</span>
                <button type="button" className={styles.secondary} disabled={busy !== null} onClick={() => void decide(item.request_id, pairingApi.reject)}>
                  Reject
                </button>
                <button type="button" className={styles.primary} disabled={busy !== null} onClick={() => void decide(item.request_id, pairingApi.approve)}>
                  Approve
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
    </section>
  );
}
