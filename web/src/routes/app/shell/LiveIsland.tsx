import { useLayoutEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { SentinalMark } from "../../../components/SentinalMark";
import { useLiveData } from "../../../data/LiveDataContext";
import { islandLocation } from "./location";
import { needCount, tickerItems } from "./ticker";
import styles from "./LiveIsland.module.css";

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/**
 * The only permanent chrome on desktop (GLASS_SPEC.md §4): the logo, where you
 * are, what needs you, and the prompt button. When the ticker does not fit it
 * collapses to an "N need you" chip.
 */
export function LiveIsland({ onOpenPrompt }: { onOpenPrompt: (prefill?: string) => void }) {
  const navigate = useNavigate();
  const { pathname, search } = useLocation();
  const { machines, sessions, terminals } = useLiveData();
  const where = islandLocation(pathname, search, machines, sessions);
  const items = tickerItems(terminals);
  const needs = needCount(terminals);

  // Measure the full ticker off-screen; collapse when it would not fit.
  const measure = useRef<HTMLSpanElement>(null);
  const slot = useRef<HTMLSpanElement>(null);
  const [collapsed, setCollapsed] = useState(false);
  useLayoutEffect(() => {
    const check = () => {
      if (!measure.current || !slot.current) return;
      const room = window.innerWidth - 32 - (slot.current.getBoundingClientRect().left - 16) - 120;
      setCollapsed(measure.current.scrollWidth > Math.min(room, 560));
    };
    check();
    window.addEventListener("resize", check);
    return () => window.removeEventListener("resize", check);
  });

  const ticker = (
    <>
      {items.map((item, index) => (
        <span key={item.sessionId} className={styles.tickerSlot}>
          {index > 0 && <span className={styles.dot} aria-hidden="true">·</span>}
          <button
            type="button"
            className={styles.tickerItem}
            data-state={item.state}
            onClick={() => navigate(`/app/sessions/${item.sessionId}`)}
          >
            <span className={styles.square} aria-hidden="true" />
            {item.text}
          </button>
        </span>
      ))}
    </>
  );

  return (
    <nav className={styles.island} aria-label="Sentinal">
      <button type="button" className={styles.logo} onClick={() => navigate("/app/overview")} title="Dashboard">
        <SentinalMark size={18} />
      </button>
      <button
        type="button"
        className={styles.location}
        onClick={() => onOpenPrompt("go ")}
        title="Go somewhere"
        aria-label={`You are on ${where.kind === "session" ? `${where.machine} › ${where.title}` : where.name}. Go somewhere`}
      >
        {where.kind === "session" ? (
          <>
            <span className={styles.machine}>{where.machine}</span>
            <span className={styles.chevron} aria-hidden="true">›</span>
            <span className={styles.title}>{where.title}</span>
          </>
        ) : (
          <span className={styles.title}>{where.name}</span>
        )}
      </button>
      {(items.length > 0 || needs > 0) && <span className={styles.divider} aria-hidden="true" />}
      <span ref={slot} className={styles.ticker} aria-live="polite" aria-label="What needs you">
        {collapsed || items.length === 0 ? (
          needs > 0 && (
            <button type="button" className={styles.needChip} onClick={() => navigate("/app/overview")}>
              {needs} need{needs === 1 ? "s" : ""} you
            </button>
          )
        ) : (
          ticker
        )}
      </span>
      <span ref={measure} className={styles.measure} aria-hidden="true">
        {ticker}
      </span>
      <button
        type="button"
        className={styles.promptButton}
        onClick={() => onOpenPrompt()}
        aria-label="Open the prompt"
        aria-keyshortcuts={IS_MAC ? "Meta+K /" : "Control+K /"}
      >
        <span className={styles.caret} aria-hidden="true">❯</span>
        <span className={styles.shortcut}>{IS_MAC ? "⌘K" : "Ctrl K"}</span>
      </button>
    </nav>
  );
}
