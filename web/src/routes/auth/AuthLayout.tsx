import type { ReactNode } from "react";

import { DotField, type DotSignal } from "../../components/DotField";
import { useAuth } from "../../context/AuthContext";
import { useSequentialTyping } from "../../hooks/useTypedText";
import { AuthIsland, type IslandTone } from "./AuthIsland";
import styles from "./AuthLayout.module.css";
import { AUTH_COPY, type AuthMode } from "./copy";

export interface AuthLayoutProps {
  mode: AuthMode;
  mark: ReactNode;
  /** What the live island says about this screen, e.g. "Signed out" or "Locked · 04:52". */
  island: { tone: IslandTone; status: ReactNode };
  /** Small print under the right column; omitted when the screen has none. */
  foot?: string;
  children: ReactNode;
  /** True while the success (grow) mark animation plays, so the ×140 scale can't cause scrollbar jank. */
  hideOverflow?: boolean;
  /** What the background dots show: checking, a wrong attempt, success. */
  signal?: DotSignal;
}

/**
 * The startup and sign-in frame: flat ground, the mark and a typed headline on
 * the left, the form or checks on the right, and the live island on top.
 */
export function AuthLayout({ mode, mark, island, foot, children, hideOverflow = false, signal = "idle" }: AuthLayoutProps) {
  const { latencyMs, reachable } = useAuth();
  const copy = AUTH_COPY[mode];
  // No resetKey: the hook already restarts when the copy text itself changes,
  // and login/wrong share identical copy so a wrong attempt doesn't re-trigger typing.
  const { revealed, done } = useSequentialTyping([copy.overline.toLowerCase(), copy.title], 45);
  const [overTyped, titleTyped] = revealed;

  return (
    <div className={styles.root} style={{ overflowY: hideOverflow ? "hidden" : "auto" }}>
      <DotField signal={signal} />
      <AuthIsland tone={island.tone} status={island.status} latencyMs={reachable ? latencyMs : null} />
      <main className={styles.main}>
        <div className={styles.frame}>
          <div className={styles.left}>
            <div className={styles.markSlot} data-dots-origin>
              {mark}
            </div>
            <div className={styles.copy}>
              <span className={styles.overline} style={{ color: copy.overlineColor }}>
                {"> "}
                {overTyped}
              </span>
              <h1 className={styles.title} aria-label={copy.title}>
                {titleTyped}
                <span className={`${styles.cursor} tg-motion`} aria-hidden="true" />
              </h1>
              <p className={styles.body} style={{ opacity: done ? 1 : 0 }}>
                {copy.body}
              </p>
            </div>
          </div>
          <div className={styles.right}>
            {children}
            {foot !== undefined && <span className={styles.foot}>{foot}</span>}
          </div>
        </div>
      </main>
    </div>
  );
}
