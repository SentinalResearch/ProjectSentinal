import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import { useAuth } from "../../context/AuthContext";
import type { AuthStatus } from "../../data/types";
import { AuthLayout } from "./AuthLayout";
import { AuthMark } from "./AuthMark";
import styles from "./StartupPage.module.css";

/** Shortest time the startup screen stays up, so a fast check doesn't flash. */
export const STARTUP_HOLD_MS = 600;

type CheckState = "done" | "pending" | "failed" | "warn" | "skipped";

const GLYPH: Record<CheckState, string> = { done: "✓", pending: "●", failed: "✕", warn: "!", skipped: "–" };

/** Where startup sends the browser once the server has answered. */
export function startupDestination(status: AuthStatus): string {
  if (status === "signedIn") return "/app/overview";
  if (status === "setup") return "/setup";
  return "/login";
}

function prefersReducedMotion(): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * `/` — the first screen: reaches the server, checks the connection and the
 * sign-in, then hands off to sign-in, first run or the app. Shows nothing
 * about machines or sessions before sign-in.
 */
export function StartupPage() {
  const { loading, reachable, status, latencyMs, retry } = useAuth();
  const navigate = useNavigate();
  const [held, setHeld] = useState(() => !prefersReducedMotion());

  useEffect(() => {
    if (!held) return;
    const timer = setTimeout(() => setHeld(false), STARTUP_HOLD_MS);
    return () => clearTimeout(timer);
  }, [held]);

  useEffect(() => {
    if (!loading && reachable && !held) navigate(startupDestination(status), { replace: true });
  }, [loading, reachable, held, status, navigate]);

  const unreachable = !loading && !reachable;
  const secure = window.isSecureContext;
  const checks: { label: string; state: CheckState; detail: string }[] = [
    {
      label: "Reaching the server",
      state: loading ? "pending" : reachable ? "done" : "failed",
      detail: loading ? "…" : reachable && latencyMs !== null ? `${latencyMs} ms` : "no answer",
    },
    {
      label: "Secure connection",
      state: loading ? "pending" : unreachable ? "skipped" : secure ? "done" : "warn",
      detail: window.location.protocol === "https:" ? "https" : secure ? "local" : "http",
    },
    {
      label: "Checking sign-in",
      state: loading ? "pending" : unreachable ? "skipped" : "done",
      detail: loading || unreachable ? "…" : "ready",
    },
  ];
  const doneCount = checks.filter((check) => check.state === "done").length;

  return (
    <AuthLayout
      mode={unreachable ? "unreachable" : "startup"}
      mark={<AuthMark mode={unreachable ? "unreachable" : "startup"} busy={loading} />}
      island={
        unreachable
          ? { tone: "off", status: "Unreachable" }
          : { tone: loading ? "pending" : "ok", status: loading ? "Connecting…" : "Connected" }
      }
      foot={unreachable ? undefined : "Nothing about your machines is shown until you sign in."}
    >
      <ol className={styles.checks} aria-label="Startup checks" aria-live="polite">
        {checks.map((check) => (
          <li key={check.label} className={styles.check}>
            <span className={`${styles.glyph} ${styles[check.state]} ${check.state === "pending" ? "tg-motion" : ""}`} aria-hidden="true">
              {GLYPH[check.state]}
            </span>
            <span className={styles.label}>{check.label}</span>
            <span className={styles.detail}>{check.detail}</span>
          </li>
        ))}
      </ol>

      {unreachable ? (
        <div className={styles.recover}>
          <div className={styles.command}>
            <span className={styles.prompt} aria-hidden="true">
              ❯
            </span>{" "}
            sentinal status
          </div>
          <button type="button" className={styles.retry} onClick={retry}>
            <span>Try again</span>
            <span className={styles.enterGlyph} aria-hidden="true">
              ↵
            </span>
          </button>
        </div>
      ) : (
        <div
          className={styles.progress}
          role="progressbar"
          aria-label="Startup progress"
          aria-valuemin={0}
          aria-valuemax={checks.length}
          aria-valuenow={doneCount}
        >
          <div className={styles.progressFill} style={{ width: `${Math.max(8, (doneCount / checks.length) * 100)}%` }} />
        </div>
      )}
    </AuthLayout>
  );
}
