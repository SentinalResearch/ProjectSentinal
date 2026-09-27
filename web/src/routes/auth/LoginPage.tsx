import { useEffect, useRef, useState, type FormEvent } from "react";

import { useAuth } from "../../context/AuthContext";
import { config } from "../../data/config";
import { isMockData } from "../../data";
import { AuthLayout } from "./AuthLayout";
import { AuthMark } from "./AuthMark";
import styles from "./LoginPage.module.css";
import type { AuthMode } from "./copy";

function modeFromStatus(status: string): AuthMode {
  if (status === "wrong" || status === "locked" || status === "expired") return status;
  return "login";
}

function formatClock(ms: number): string {
  const totalSeconds = Math.max(0, Math.ceil(ms / 1000));
  const mm = String(Math.floor(totalSeconds / 60)).padStart(2, "0");
  const ss = String(totalSeconds % 60).padStart(2, "0");
  return `${mm}:${ss}`;
}

const FOOT = "Signed-in browsers stay signed in until you sign out or the session expires.";

export function LoginPage() {
  const { status, attempts, lockUntil, login, clearLock } = useAuth();
  const mode = modeFromStatus(status);

  const [pw, setPw] = useState("");
  const [show, setShow] = useState(false);
  const [busy, setBusy] = useState(false);
  const [ok, setOk] = useState(false);
  const [shaking, setShaking] = useState(false);
  const prevAttempts = useRef(attempts);

  useEffect(() => {
    if (mode === "wrong" && attempts !== prevAttempts.current) {
      setShaking(true);
      const t = setTimeout(() => setShaking(false), 450);
      prevAttempts.current = attempts;
      return () => clearTimeout(t);
    }
    prevAttempts.current = attempts;
  }, [mode, attempts]);

  // Clear the password field whenever the mode changes into a fresh attempt.
  useEffect(() => {
    setPw("");
  }, [mode]);

  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (status !== "locked") return;
    const interval = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(interval);
  }, [status]);

  useEffect(() => {
    if (status === "locked" && lockUntil !== null && lockUntil - now <= 0) {
      clearLock();
    }
  }, [status, lockUntil, now, clearLock]);

  const locked = status === "locked";
  const attemptsLeft = Math.max(0, config.loginMaxAttempts - attempts);

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    if (locked || busy || ok || pw.length === 0) return;
    setBusy(true);
    const result = await login(pw);
    setBusy(false);
    if (result.status === "signedIn") {
      setOk(true);
    }
  }

  return (
    <AuthLayout
      mode={mode}
      mark={<AuthMark mode={mode} busy={busy} ok={ok} shaking={shaking} />}
      foot={FOOT}
      hideOverflow={ok}
    >
      <form className={styles.form} onSubmit={handleSubmit}>
        {mode === "expired" && (
          <div className={styles.expiredBanner}>
            <span>Returning to</span>
            <span>Overview</span>
          </div>
        )}

        <div className={styles.field}>
          <span className={styles.fieldHead}>
            <label htmlFor="login-password">{isMockData ? "Password" : "Daemon token"}</label>
            <button type="button" className={styles.showToggle} onClick={() => setShow((s) => !s)}>
              {show ? "Hide" : "Show"}
            </button>
          </span>
          <span className={styles.inputWrap}>
            <svg
              width="16"
              height="16"
              viewBox="0 0 16 16"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              className={styles.inputIcon}
              aria-hidden="true"
            >
              <rect x="3" y="7" width="10" height="7" />
              <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" />
            </svg>
            <input
              id="login-password"
              type={show ? "text" : "password"}
              value={pw}
              onChange={(e) => setPw(e.target.value)}
              disabled={locked}
              autoFocus
              autoComplete="current-password"
              placeholder={locked ? "Locked" : ""}
              className={`${styles.passwordInput} ${mode === "wrong" ? styles.error : ""}`}
            />
          </span>
          {mode === "wrong" && (
            <span className={styles.errorText}>
              {isMockData
                ? `Incorrect password. ${attemptsLeft} ${attemptsLeft === 1 ? "attempt" : "attempts"} left before a 5-minute lock.`
                : "The daemon rejected that token or could not be reached."}
            </span>
          )}
        </div>

        {locked && lockUntil !== null && (
          <div className={styles.lockRow}>
            <span className={styles.lockLabel}>Try again in</span>
            <span className={styles.lockClock}>{formatClock(lockUntil - now)}</span>
          </div>
        )}

        <button
          type="submit"
          disabled={locked || pw.length === 0}
          className={`${styles.submit} ${busy ? styles.checking : ""}`}
        >
          <span>{busy ? "Checking…" : "Sign in"}</span>
          <span className={styles.enterGlyph}>↵</span>
        </button>
      </form>
    </AuthLayout>
  );
}
