import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { DevBadge } from "../../../components/DevBadge";
import { SentinalMark } from "../../../components/SentinalMark";
import { ThemeSwitch } from "../../../components/ThemeSwitch";
import { dataAdapter } from "../../../data";
import { answerPrompt } from "../../../data/actions";
import { useLiveData } from "../../../data/LiveDataContext";
import type { Session } from "../../../data/types";
import { islandLocation } from "./location";
import { PlacesMenu } from "./PlacesMenu";
import { islandFocus, needCount, tickerItems } from "./ticker";
import styles from "./LiveIsland.module.css";

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/**
 * The only permanent chrome on desktop (GLASS_SPEC.md §4): the logo, where you
 * are, what needs you, and the prompt button. When the ticker does not fit it
 * collapses to an "N need you" chip.
 */
export function LiveIsland({
  onOpenPrompt,
  interrupting = null,
  onCloseInterrupt,
}: {
  onOpenPrompt: (prefill?: string) => void;
  /** A Kill… waiting for confirmation: the island asks before sending Ctrl-C. */
  interrupting?: Session | null;
  onCloseInterrupt?: () => void;
}) {
  const navigate = useNavigate();
  const { pathname, search } = useLocation();
  const { machines, sessions, terminals } = useLiveData();
  const where = islandLocation(pathname, search, machines, sessions);
  const items = tickerItems(terminals);
  const needs = needCount(terminals);
  // Questions set aside with "Not now" go back to the ticker.
  const [setAside, setSetAside] = useState<ReadonlySet<string>>(() => new Set());
  const focus = islandFocus(terminals, setAside);
  const machineName = (id: string) => machines.find((machine) => machine.id === id)?.name ?? id;

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
      <button type="button" className={styles.logo} onClick={() => navigate("/app/overview")} title="Overview">
        <SentinalMark size={18} />
      </button>
      <DevBadge />
      <PlacesMenu
        triggerClass={styles.location}
        onSearch={() => onOpenPrompt()}
        label={`You are on ${where.kind === "session" ? `${where.machine} › ${where.title}` : where.name}. Go somewhere`}
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
      </PlacesMenu>
      {(interrupting || focus.kind !== "idle" || items.length > 0) && <span className={styles.divider} aria-hidden="true" />}
      {interrupting ? (
        <IslandConfirm session={interrupting} machineName={machineName(interrupting.machine)} onClose={() => onCloseInterrupt?.()} />
      ) : focus.kind === "answer" ? (
        <span className={styles.answer} role="group" aria-label={`${focus.session.name} asks`}>
          <button type="button" className={styles.answerName} onClick={() => navigate(`/app/sessions/${focus.session.id}`)}>
            {`${focus.session.name} asks`}
          </button>
          <span className={styles.answerPrompt}>{focus.session.prompt}</span>
          <button type="button" className={styles.answerYes} onClick={() => void answerPrompt(focus.session.id, "y")} aria-label={`y, answer ${focus.session.name}`}>
            y
          </button>
          <button type="button" className={styles.answerNo} onClick={() => void answerPrompt(focus.session.id, "n")} aria-label={`n, answer ${focus.session.name}`}>
            n
          </button>
          <button
            type="button"
            className={styles.answerLater}
            onClick={() => setSetAside((previous) => new Set(previous).add(focus.session.id))}
            aria-label={`Not now, keep ${focus.session.name} in the list`}
            title="Not now"
          >
            ×
          </button>
        </span>
      ) : focus.kind === "progress" ? (
        <button
          type="button"
          className={styles.progress}
          onClick={() => navigate(`/app/sessions/${focus.session.id}`)}
          aria-label={`${focus.session.name}, ${focus.percent} % done`}
        >
          <ProgressRing percent={focus.percent} />
          <span className={styles.progressName}>{focus.session.name}</span>
          <span className={styles.progressPercent}>{focus.percent} %</span>
          {focus.session.progress && (
            <span className={styles.progressDetail}>
              {focus.session.progress.unit} {focus.session.progress.current}/{focus.session.progress.total}
            </span>
          )}
        </button>
      ) : (
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
      )}
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
        <svg className={styles.searchIcon} width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
          <circle cx="11" cy="11" r="7" />
          <path d="M20 20l-3.5-3.5" />
        </svg>
        <span className={styles.searchText} aria-hidden="true">Search or type a command</span>
        <span className={styles.shortcut}>{IS_MAC ? "⌘K" : "Ctrl K"}</span>
      </button>
      <ThemeSwitch />
    </nav>
  );
}

/** A small ring that fills with the job's progress. */
function ProgressRing({ percent }: { percent: number }) {
  const circumference = 2 * Math.PI * 8;
  return (
    <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden="true" className={styles.ring}>
      <circle cx="10" cy="10" r="8" fill="none" stroke="var(--line)" strokeWidth="2.5" />
      <circle
        cx="10"
        cy="10"
        r="8"
        fill="none"
        stroke="var(--accent)"
        strokeWidth="2.5"
        strokeLinecap="round"
        strokeDasharray={`${(circumference * Math.min(100, Math.max(0, percent))) / 100} ${circumference}`}
        transform="rotate(-90 10 10)"
      />
    </svg>
  );
}

/**
 * Kill… confirmed in place (nothing interrupts on one click). Focus starts on
 * Cancel, the safe choice; Escape cancels.
 */
function IslandConfirm({ session, machineName, onClose }: { session: Session; machineName: string; onClose: () => void }) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    cancel.current?.focus();
  }, [session.id]);
  return (
    <span
      className={styles.confirm}
      role="alertdialog"
      aria-label={`Send Ctrl-C to ${session.name}?`}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <span className={styles.confirmQuestion}>Kill {session.name}?</span>
      <span className={styles.confirmDetail}>sends Ctrl-C on {machineName}</span>
      <button ref={cancel} type="button" className={styles.confirmCancel} onClick={onClose}>
        Cancel
      </button>
      <button
        type="button"
        className={styles.confirmKill}
        onClick={() => {
          void dataAdapter.sendSignal(session.id, "SIGINT");
          onClose();
        }}
      >
        Send Ctrl-C
      </button>
    </span>
  );
}
