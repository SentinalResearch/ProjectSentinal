import { useEffect, useRef, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";

import { DevBadge } from "../../../components/DevBadge";
import { SentinalMark } from "../../../components/SentinalMark";
import { ThemeSwitch } from "../../../components/ThemeSwitch";
import { useLiveData } from "../../../data/LiveDataContext";
import { islandLocation } from "../shell/location";
import { PlacesMenu } from "../shell/PlacesMenu";
import { IslandInbox, useUnreadCount } from "../shell/IslandInbox";
import { OPEN_INBOX_EVENT } from "../shell/usePromptActions";
import { tickerItems } from "../shell/ticker";
import type { IslandBack } from "./islandBack";
import styles from "./MobileIsland.module.css";

/**
 * The phone island (MOBILE_GLASS_SPEC §1): logo · page · the one most urgent
 * live item · the needs-you count. On a detail screen: ‹ back, the title, and
 * the screen's own trailing control. Over the terminal it is `docked`: a
 * flat bar above the terminal instead of glass floating on it.
 */
export function MobileIsland({ back, docked = false }: { back: IslandBack | null; docked?: boolean }) {
  const navigate = useNavigate();
  const { pathname, search, key } = useLocation();
  const { machines, sessions, terminals } = useLiveData();
  const where = islandLocation(pathname, search, machines, sessions);
  const [top] = tickerItems(terminals, 1);
  const unread = useUnreadCount(terminals);
  const [inboxOpen, setInboxOpen] = useState(false);
  useEffect(() => {
    const open = () => setInboxOpen(true);
    window.addEventListener(OPEN_INBOX_EVENT, open);
    return () => window.removeEventListener(OPEN_INBOX_EVENT, open);
  }, []);
  const islandRef = useRef<HTMLElement>(null);
  const inboxRef = useRef<HTMLDivElement>(null);
  const machineName = (id: string) => machines.find((machine) => machine.id === id)?.name ?? id;
  // A tap outside the island and its inbox closes the inbox.
  useEffect(() => {
    if (!inboxOpen) return;
    const away = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!islandRef.current?.contains(target) && !inboxRef.current?.contains(target)) setInboxOpen(false);
    };
    document.addEventListener("pointerdown", away);
    return () => document.removeEventListener("pointerdown", away);
  }, [inboxOpen]);

  if (back) {
    const session = where.kind === "session" ? sessions.find((entry) => entry.id === where.sessionId) : undefined;
    const title = where.kind === "session" ? where.title : where.name;
    const subtitle = where.kind === "session" ? [where.machine, session?.cwd].filter(Boolean).join(" · ") : null;
    const goBack = () => (key !== "default" ? navigate(-1) : navigate(back.parentPath, { replace: true }));
    return (
      <nav className={`${styles.island} island-ink`} aria-label="Sentinal" data-docked={docked || undefined}>
        <button type="button" className={styles.back} onClick={goBack} aria-label={`Back to ${back.parentLabel}`}>
          <span aria-hidden="true">‹</span>
        </button>
        <span className={styles.titles}>
          <span className={styles.title}>{title}</span>
          {subtitle && <span className={styles.subtitle}>{subtitle}</span>}
        </span>
        <DevBadge />
        {back.trailing}
      </nav>
    );
  }

  const name = pathname.startsWith("/app/split") ? "Terminals" : where.kind === "page" ? where.name : where.title;
  return (
    <>
    <nav ref={islandRef} className={`${styles.island} island-ink`} aria-label="Sentinal" data-docked={docked || undefined}>
      <Link to="/app/overview" className={styles.logo} aria-label="Overview">
        <SentinalMark size={18} />
      </Link>
      <DevBadge />
      <PlacesMenu label={`You are on ${name}. Go somewhere`} triggerClass={styles.page}>
        <span>{name}</span>
        <span className={styles.caret} aria-hidden="true">▾</span>
      </PlacesMenu>
      {top && (
        <Link to={`/app/sessions/${encodeURIComponent(top.sessionId)}`} className={styles.ticker} data-state={top.state}>
          <span className={styles.square} aria-hidden="true" />
          <span className={styles.tickerText}>{top.text}</span>
        </Link>
      )}
      {(unread > 0 || inboxOpen) && (
        <button
          type="button"
          className={styles.unread}
          aria-expanded={inboxOpen}
          aria-label={unread > 0 ? `${unread} unread notification${unread === 1 ? "" : "s"}` : "Notifications"}
          onClick={() => setInboxOpen((open) => !open)}
        >
          {unread > 0 ? unread : "×"}
        </button>
      )}
      <ThemeSwitch />
    </nav>
    {inboxOpen && (
      <div ref={inboxRef} className={`${styles.inboxPanel} island-ink`}>
        <IslandInbox terminals={terminals} machineName={machineName} onClose={() => setInboxOpen(false)} />
      </div>
    )}
    </>
  );
}
