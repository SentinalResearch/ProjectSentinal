import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";
import { useLocation, useNavigate } from "react-router-dom";

import { DevBadge } from "../../../components/DevBadge";
import { SentinalMark } from "../../../components/SentinalMark";
import { ThemeChoice } from "../../../components/ThemeSwitch";
import { dataAdapter } from "../../../data";
import { answerPrompt, endSession } from "../../../data/actions";
import { useLiveData } from "../../../data/LiveDataContext";
import type { Session } from "../../../data/types";
import { IslandInbox, useUnreadCount } from "./IslandInbox";
import { islandLocation } from "./location";
import { PlacesMenu } from "./PlacesMenu";
import type { BackEntry } from "./SheetStage";
import { islandFocus, needCount, tickerItems } from "./ticker";
import styles from "./LiveIsland.module.css";

const FINISHED: ReadonlySet<string> = new Set(["COMPLETED", "FAILED", "INTERRUPTED"]);

const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

/**
 * The only permanent chrome on desktop (GLASS_SPEC.md §4): the logo, where you
 * are, what needs you, and the prompt button. When the ticker does not fit it
 * collapses to an "N need you" chip.
 */
export function LiveIsland({
  onOpenPrompt,
  interrupting = null,
  ending = false,
  onCloseInterrupt,
  back = [],
}: {
  onOpenPrompt: (prefill?: string) => void;
  /** A Kill… waiting for confirmation: the island asks before sending Ctrl-C. */
  interrupting?: Session | null;
  /** The confirm ends the terminal (Ctrl-C, then close its shell) instead of only Ctrl-C. */
  ending?: boolean;
  onCloseInterrupt?: () => void;
  /** Pages you can go back to, newest first. */
  back?: BackEntry[];
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
  const islandRef = useRef<HTMLElement>(null);
  const unread = useUnreadCount(terminals);
  const [inboxOpen, setInboxOpen] = useState(false);
  // A click outside the island closes the inbox.
  useEffect(() => {
    if (!inboxOpen) return;
    const away = (event: PointerEvent) => {
      if (!islandRef.current?.contains(event.target as Node)) setInboxOpen(false);
    };
    document.addEventListener("pointerdown", away);
    return () => document.removeEventListener("pointerdown", away);
  }, [inboxOpen]);
  const latestSessions = useRef(sessions);
  latestSessions.current = sessions;
  const stillLive = (id: string) => {
    const state = latestSessions.current.find((session) => session.id === id)?.state;
    return state !== undefined && !FINISHED.has(state);
  };
  const anchorX = useIslandAnchor(pathname, islandRef);

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
            title={item.text}
          >
            <span className={styles.square} aria-hidden="true" />
            <span className={styles.tickerText}>{item.text}</span>
          </button>
        </span>
      ))}
    </>
  );

  return (
    <nav
      ref={islandRef}
      className={`${styles.island} island-ink`}
      style={anchorX === null ? undefined : { left: `${anchorX}px` }}
      aria-label="Sentinal"
      title="Click or press / to search"
      onClick={(event) => {
        // The island is the search: a click on its body (not on one of its controls) opens it.
        if (!(event.target as HTMLElement).closest("button, a, [role='group'], [role='alertdialog']")) onOpenPrompt();
      }}
    >
      <div className={styles.row}>
      <button type="button" className={styles.logo} onClick={() => navigate("/app/overview")} title="Overview">
        <SentinalMark size={18} />
      </button>
      <DevBadge />
      {back.length > 0 && <BackPill back={back} onGo={(entry) => navigate(entry.href)} />}
      <PlacesMenu
        triggerClass={styles.location}
        onSearch={() => onOpenPrompt()}
        footer={<ThemeChoice />}
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
      {(focus.kind !== "idle" || items.length > 0 || needs > 0) && <span className={styles.divider} aria-hidden="true" />}
      {focus.kind === "progress" && !interrupting ? (
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
      {(unread > 0 || inboxOpen) && (
        <button
          type="button"
          className={styles.unread}
          aria-expanded={inboxOpen}
          aria-label={unread > 0 ? `${unread} unread notification${unread === 1 ? "" : "s"}` : "Notifications"}
          title="Notifications"
          onClick={() => setInboxOpen((open) => !open)}
        >
          {unread > 0 ? unread : "×"}
        </button>
      )}
      <button
        type="button"
        className={styles.promptButton}
        onClick={() => onOpenPrompt()}
        aria-label="Open the prompt"
        aria-keyshortcuts={IS_MAC ? "Meta+K /" : "Control+K /"}
      >
        /
      </button>
      </div>
      {/* Grows down, never sideways: a question or a Kill confirm drops in under the row. */}
      {(interrupting || inboxOpen || focus.kind === "answer") && (
        <div className={styles.drop} data-inbox={(inboxOpen && !interrupting) || undefined}>
          {interrupting ? (
            <IslandConfirm
              session={interrupting}
              machineName={machineName(interrupting.machine)}
              ending={ending}
              stillLive={() => stillLive(interrupting.id)}
              onClose={() => onCloseInterrupt?.()}
            />
          ) : inboxOpen ? (
            <IslandInbox terminals={terminals} machineName={machineName} onClose={() => setInboxOpen(false)} />
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
          ) : null}
        </div>
      )}
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
function IslandConfirm({
  session,
  machineName,
  ending,
  stillLive,
  onClose,
}: {
  session: Session;
  machineName: string;
  ending: boolean;
  stillLive: () => boolean;
  onClose: () => void;
}) {
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    cancel.current?.focus();
  }, [session.id]);
  const question = ending ? `End ${session.name}?` : `Kill ${session.name}?`;
  return (
    <span
      className={styles.confirm}
      data-ending={ending || undefined}
      role="alertdialog"
      aria-label={ending ? `End ${session.name}?` : `Send Ctrl-C to ${session.name}?`}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          onClose();
        }
      }}
    >
      <span className={styles.confirmQuestion}>{question}</span>
      <span className={styles.confirmDetail}>
        {ending ? `Ctrl-C, then closes the terminal on ${machineName}. Its output is kept.` : `sends Ctrl-C on ${machineName}`}
      </span>
      <button ref={cancel} type="button" className={styles.confirmCancel} onClick={onClose}>
        {ending ? "Keep running" : "Cancel"}
      </button>
      <button
        type="button"
        className={styles.confirmKill}
        onClick={() => {
          if (ending) void endSession(session.id, stillLive).catch(() => undefined);
          else void dataAdapter.sendSignal(session.id, "SIGINT");
          onClose();
        }}
      >
        {ending ? "End" : "Send Ctrl-C"}
      </button>
    </span>
  );
}

/**
 * Where the island centres itself: over the current page's `data-island-anchor`
 * element when it has one (the Session page's details side, so the island
 * never sits on the terminal), else the window's centre (null). Pages mount
 * after the island and move on resize, so it keeps measuring: after every
 * render, on resize, and on a light timer; it only updates when the spot moves.
 */
function useIslandAnchor(pathname: string, island: RefObject<HTMLElement | null>): number | null {
  const [x, setX] = useState<number | null>(null);
  const [, setTick] = useState(0);
  const measure = () => {
    const anchor = document.querySelector("[data-current] [data-island-anchor]");
    const box = anchor?.getBoundingClientRect();
    let next = box && box.width > 0 ? box.left + box.width / 2 : null;
    // Never past an edge: keep the whole island inside the window, 16 px in.
    if (next !== null) {
      const half = (island.current?.offsetWidth ?? 0) / 2;
      next = Math.round(Math.min(window.innerWidth - 16 - half, Math.max(16 + half, next)));
    }
    setX((previous) => (previous === next ? previous : next));
  };
  useLayoutEffect(measure);
  useEffect(() => {
    const again = () => setTick((tick) => tick + 1);
    window.addEventListener("resize", again);
    const timer = setInterval(measure, 500);
    return () => {
      window.removeEventListener("resize", again);
      clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pathname]);
  return x;
}

/**
 * Going back, in the island instead of on the page's edge: "‹ Recipes" goes
 * back one (⌘[ too); hover or focus shows the earlier pages.
 */
function BackPill({ back, onGo }: { back: BackEntry[]; onGo: (entry: BackEntry) => void }) {
  const [newest, ...earlier] = back;
  return (
    <span className={styles.back}>
      <button
        type="button"
        className={styles.backPill}
        onClick={() => onGo(newest)}
        aria-label={`Back to ${newest.label}`}
        aria-keyshortcuts="Meta+BracketLeft"
        title={`Back to ${newest.label} (⌘[)`}
      >
        {`‹ ${newest.label}`}
      </button>
      {earlier.length > 0 && (
        <span className={styles.backMenu} role="group" aria-label="Earlier pages">
          {earlier.slice(0, 4).map((entry) => (
            <button key={entry.key} type="button" className={styles.backItem} onClick={() => onGo(entry)} aria-label={`Back to ${entry.label}`}>
              {entry.state ? <span className={styles.backDot} data-state={entry.state} aria-hidden="true" /> : <span className={styles.backDot} aria-hidden="true" />}
              {entry.label}
            </button>
          ))}
        </span>
      )}
    </span>
  );
}
