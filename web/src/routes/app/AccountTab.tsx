import { useState, type FormEvent } from "react";

import { useAuth } from "../../context/AuthContext";
import { dataAdapter } from "../../data";
import { config } from "../../data/config";
import styles from "./SettingsPage.module.css";

/**
 * The multi-browser session list isn't built here — per AGENT_PROMPT.md, "only
 * 'This browser' is shown now" is the actual current design (SentinalSettings.dc.html
 * shows exactly one row), not a placeholder to be filled with invented mock rows.
 * `flags.multiBrowserSessions` stays unused by this tab for that reason.
 */
export function AccountTab() {
  const { logout, markExpired } = useAuth();
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [confirm, setConfirm] = useState("");
  const [message, setMessage] = useState<{ text: string; ok: boolean } | null>(null);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (!current) {
      setMessage({ text: "Enter your current password.", ok: false });
      return;
    }
    if (next.length < config.setupMinPasswordLength) {
      setMessage({ text: `New password needs at least ${config.setupMinPasswordLength} characters.`, ok: false });
      return;
    }
    if (next !== confirm) {
      setMessage({ text: "New passwords don't match.", ok: false });
      return;
    }
    try {
      await dataAdapter.auth.changePassword(current, next);
      setCurrent("");
      setNext("");
      setConfirm("");
      setMessage({ text: "Password updated. Other browsers will be signed out.", ok: true });
    } catch (err) {
      setMessage({ text: err instanceof Error ? err.message : "Could not update password.", ok: false });
    }
  }

  return (
    <>
      <header className={styles.tabHeader}>
        <div className={styles.tabHeaderText}>
          <h2 className={styles.tabTitle}>Account</h2>
          <span className={styles.tabSubtitle}>One admin account. Signed-in browsers are listed below.</span>
        </div>
      </header>

      <form onSubmit={handleSubmit} className={styles.pwForm}>
        <span className={styles.formLabel}>Change password</span>
        <label className={styles.pwField}>
          <span className={styles.fieldLabel}>Current password</span>
          <input
            type="password"
            value={current}
            onChange={(e) => {
              setCurrent(e.target.value);
              setMessage(null);
            }}
            className={styles.pwInput}
          />
        </label>
        <label className={styles.pwField}>
          <span className={styles.fieldLabel}>New password</span>
          <input
            type="password"
            value={next}
            onChange={(e) => {
              setNext(e.target.value);
              setMessage(null);
            }}
            className={styles.pwInput}
          />
        </label>
        <label className={styles.pwField}>
          <span className={styles.fieldLabel}>Confirm new password</span>
          <input
            type="password"
            value={confirm}
            onChange={(e) => {
              setConfirm(e.target.value);
              setMessage(null);
            }}
            className={styles.pwInput}
          />
        </label>
        {message && (
          <span className={styles.pwMessage} style={{ color: message.ok ? "var(--s-running-fg)" : "var(--s-failed-fg)" }}>
            {message.text}
          </span>
        )}
        <button type="submit" className={styles.pwSubmit}>
          Update password
        </button>
      </form>

      <div className={styles.section}>
        <span className={styles.sectionHeading}>Signed-in browsers</span>
        <div className={styles.browserRow}>
          <div className={styles.browserCol}>
            <span className={styles.machineName}>This browser</span>
            <span className={styles.machineOs}>Active now</span>
          </div>
          <button type="button" className={styles.smallBtn} onClick={() => void logout()}>
            Sign out
          </button>
        </div>
      </div>

      <div className={styles.prototypeRow}>
        <span className={styles.prototypeLabel}>PROTOTYPE</span>
        <button type="button" className={styles.smallBtn} onClick={markExpired}>
          Simulate session expiry
        </button>
      </div>
    </>
  );
}
