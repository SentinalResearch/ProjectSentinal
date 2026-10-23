import { useCallback, useEffect, useRef, useState } from "react";

import { BottomSheet } from "../../components/BottomSheet";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { releaseApi } from "../../data/api/release";
import { useIsPhone } from "../../hooks/useViewport";
import type { ControlReleaseView } from "../../protocol/ControlReleaseView";
import type { ReleaseChannel } from "../../protocol/ReleaseChannel";
import type { ReleasePolicy } from "../../protocol/ReleasePolicy";
import styles from "./ControlReleaseCard.module.css";

const POLICIES: { value: ReleasePolicy; label: string }[] = [
  { value: "NOTIFY", label: "Notify" },
  { value: "AUTOMATIC_WHEN_IDLE", label: "Automatic when idle" },
  { value: "OFF", label: "Off" },
];
const CHANNELS: { value: ReleaseChannel; label: string }[] = [
  { value: "STABLE", label: "Stable" },
  { value: "BETA", label: "Beta" },
];

function ago(at: number | null): string {
  if (at === null) return "never checked";
  const minutes = Math.max(0, Math.round((Date.now() - at) / 60_000));
  if (minutes < 2) return "checked just now";
  if (minutes < 120) return `checked ${minutes} min ago`;
  return `checked ${Math.round(minutes / 60)} h ago`;
}

function sessionsText(count: number): string {
  if (count === 0) return "No live terminals run on this control.";
  return `This ends ${count} live terminal${count === 1 ? "" : "s"} on this control.`;
}

/**
 * The control's own release (MILESTONE 4.E §11b): installed version, the
 * newest release signed by a pinned key, Check now, the update policy, and a
 * confirmed one-button update that survives the dashboard reconnecting.
 */
export function ControlReleaseCard({ pollMs = 3_000 }: { pollMs?: number }) {
  const [release, setRelease] = useState<ControlReleaseView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [reconnecting, setReconnecting] = useState(false);
  const loaded = useRef(false);
  const isPhone = useIsPhone();

  const refresh = useCallback(async () => {
    try {
      const value = await releaseApi.view();
      loaded.current = true;
      setRelease(value);
      setReconnecting(false);
    } catch {
      // Before the first load this is not a control; afterwards it is restarting.
      if (loaded.current) setReconnecting(true);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const running = release?.update?.state === "RUNNING";
  useEffect(() => {
    if (!running && !reconnecting) return;
    const timer = window.setInterval(() => void refresh(), pollMs);
    return () => window.clearInterval(timer);
  }, [running, reconnecting, pollMs, refresh]);

  if (!release) return null;
  const offer = release.available;
  const update = release.update;

  async function act(action: () => Promise<ControlReleaseView | unknown>, refreshAfter = true) {
    setBusy(true);
    setError(null);
    try {
      const value = await action();
      if (value && typeof value === "object" && "current" in value) setRelease(value as ControlReleaseView);
      else if (refreshAfter) await refresh();
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "The request failed.");
    } finally {
      setBusy(false);
    }
  }

  function startUpdate() {
    if (!offer) return;
    setConfirming(false);
    void act(() => releaseApi.update(offer.version, release!.live_full_sessions > 0));
  }

  const question = offer ? `Update this control to ${offer.version}?` : "";
  const consequence = (
    <>
      <span>The dashboard reconnects in about 30 s. Workers keep running and reconnect on their own.</span>{" "}
      <span>{sessionsText(release.live_full_sessions)}</span>{" "}
      <span>If the new release fails its health check, the control rolls back automatically.</span>
    </>
  );

  return (
    <section className={styles.card} aria-label="Control release">
      <div className={styles.head}>
        <div className={styles.headText}>
          <span className={styles.overline}>CONTROL RELEASE</span>
          <span className={styles.version}>
            {release.current} · {release.channel === "BETA" ? "beta" : "stable"}
          </span>
        </div>
        <button type="button" className={styles.secondary} disabled={busy || running} onClick={() => void act(releaseApi.check)}>
          Check now
        </button>
      </div>

      {running || reconnecting ? (
        <p className={styles.status} role="status">
          Updating to {update?.version ?? "the new release"}… the dashboard will reconnect when the control is back.
        </p>
      ) : update?.state === "UPDATED" && update.version === release.current ? (
        <p className={styles.status} role="status">
          Updated to {update.version}.
        </p>
      ) : update && update.state !== "UPDATED" && update.state !== "RUNNING" ? (
        <p className={styles.problem} role="alert">
          {update.state === "ROLLED_BACK"
            ? `Rolled back to ${update.previous}: ${update.reason ?? "the update failed its health check"}.`
            : update.state === "UNKNOWN"
              ? `No result for the update to ${update.version}; check the control with sentinal doctor.`
              : `The update to ${update.version} failed: ${update.reason ?? "see the control's release-update.log"}.`}
        </p>
      ) : null}

      {offer && !running && !reconnecting ? (
        <div className={styles.offer}>
          <span className={styles.offerText}>
            <strong>{offer.version} available</strong> · signed by a trusted key
          </span>
          {offer.notes_url && (
            <a className={styles.link} href={offer.notes_url} target="_blank" rel="noreferrer">
              Release notes
            </a>
          )}
          <button type="button" className={styles.primary} disabled={busy} onClick={() => setConfirming(true)}>
            Update control
          </button>
        </div>
      ) : !running && !reconnecting ? (
        <p className={styles.muted}>
          {release.last_checked_at === null
            ? "Not checked for newer releases yet."
            : `This control is up to date · ${ago(release.last_checked_at)}`}
        </p>
      ) : null}

      {release.unsigned_found && (
        <p className={styles.warning}>
          {release.unsigned_found} found but not signed by a trusted key; it is not offered.
        </p>
      )}
      {error && (
        <p className={styles.problem} role="alert">
          {error}
        </p>
      )}

      <div className={styles.settings}>
        <div className={styles.segmented} role="radiogroup" aria-label="Update policy">
          {POLICIES.map((option) => (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={release.policy === option.value}
              className={styles.segment}
              disabled={busy}
              onClick={() => void act(() => releaseApi.setPolicy(option.value, release.channel))}
            >
              {option.label}
            </button>
          ))}
        </div>
        <div className={styles.segmented} role="radiogroup" aria-label="Release channel">
          {CHANNELS.map((option) => (
            <button
              key={option.value}
              type="button"
              role="radio"
              aria-checked={release.channel === option.value}
              className={styles.segment}
              disabled={busy}
              onClick={() => void act(() => releaseApi.setPolicy(release.policy, option.value))}
            >
              {option.label}
            </button>
          ))}
        </div>
        <p className={styles.muted}>
          {release.policy === "AUTOMATIC_WHEN_IDLE"
            ? `Updates install between ${release.window} when no live terminal runs.`
            : release.policy === "OFF"
              ? "No automatic checks; use Check now."
              : `Checks daily and notifies you · maintenance window ${release.window}.`}
        </p>
      </div>

      {offer &&
        (isPhone ? (
          <BottomSheet open={confirming} title={question} onClose={() => setConfirming(false)}>
            <p className={styles.sheetText}>{consequence}</p>
            <button type="button" className={styles.primaryWide} onClick={startUpdate}>
              Update to {offer.version}
            </button>
            <button type="button" className={styles.secondaryWide} onClick={() => setConfirming(false)}>
              Cancel
            </button>
          </BottomSheet>
        ) : (
          <ConfirmDialog
            open={confirming}
            overline="UPDATE CONTROL"
            question={question}
            consequence={consequence}
            confirmLabel={`Update to ${offer.version}`}
            onConfirm={startUpdate}
            onCancel={() => setConfirming(false)}
          />
        ))}
    </section>
  );
}
