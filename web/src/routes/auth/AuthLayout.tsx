import type { ReactNode } from "react";
import { useNavigate } from "react-router-dom";

import { useTheme } from "../../context/ThemeContext";
import { useSequentialTyping } from "../../hooks/useTypedText";
import styles from "./AuthLayout.module.css";
import { AUTH_COPY, type AuthMode } from "./copy";

export interface AuthLayoutProps {
  mode: AuthMode;
  mark: ReactNode;
  foot: string;
  children: ReactNode;
  /** True while the success (grow) mark animation plays, so the ×140 scale can't cause scrollbar jank. */
  hideOverflow?: boolean;
}

export function AuthLayout({ mode, mark, foot, children, hideOverflow = false }: AuthLayoutProps) {
  const navigate = useNavigate();
  const { theme, toggleTheme } = useTheme();
  const copy = AUTH_COPY[mode];
  // No resetKey: the hook already restarts when the copy text itself changes,
  // and login/wrong share identical copy so a wrong attempt doesn't re-trigger typing.
  const { revealed, done } = useSequentialTyping([copy.overline.toLowerCase(), copy.title], 45);
  const [overTyped, titleTyped] = revealed;

  return (
    <div className={styles.root} style={{ overflowY: hideOverflow ? "hidden" : "auto" }}>
      <div className={styles.dotGrid} aria-hidden="true" />
      <header className={styles.header}>
        <button type="button" className={styles.headerBtn} onClick={() => navigate("/")}>
          ← Home
        </button>
        <button type="button" className={styles.headerBtn} onClick={toggleTheme}>
          {theme === "light" ? "Dark theme" : "Light theme"}
        </button>
      </header>
      <main className={styles.main}>
        <div className={styles.frame}>
          <div className={styles.left}>
            <div className={styles.markSlot}>{mark}</div>
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
            <span className={styles.foot}>{foot}</span>
          </div>
        </div>
      </main>
    </div>
  );
}
